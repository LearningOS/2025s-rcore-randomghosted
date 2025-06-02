//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::cell::RefMut;

/// Process Control Block
pub struct ProcessControlBlock {
    /// immutable
    pub pid: PidHandle,
    /// mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner of Process Control Block
pub struct ProcessControlBlockInner {
    /// is zombie?
    pub is_zombie: bool,
    /// memory set(address space)
    pub memory_set: MemorySet,
    /// parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// children process
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// exit code
    pub exit_code: i32,
    /// file descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// signal flags
    pub signals: SignalFlags,
    /// tasks(also known as threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// mutex list
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// semaphore list
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// condvar list
    pub condvar_list: Vec<Option<Arc<Condvar>>>,

    /// is allowed to detect deadlock?
    pub enable_detect_deadlock: bool,

    /// available sources for mutex
    pub available_mutex: Vec<u8>,
    /// available sources for semaphore
    pub available_semaphore: Vec<u8>,
    /// need matrix for mutex
    pub need_matrix_for_mutex: BTreeMap<usize, BTreeMap<usize,u8>>,
    /// need matrix for semaphore
    pub need_matrix_for_semaphore: BTreeMap<usize, BTreeMap<usize,u8>>,
    /// allocation matrix for mutex
    pub alloc_matrix_for_mutex: BTreeMap<usize, BTreeMap<usize, u8>>,
    /// allocation matrix for semaphore
    pub alloc_matrix_for_semaphore: BTreeMap<usize, BTreeMap<usize,u8>>,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// allocate a new file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// allocate a new task id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// deallocate a task id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// the count of tasks(threads) in this process
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// get a task with tid in this process
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }
    /// set if enable the deadlock detect
    pub fn set_deadlock_detect(&mut self, enabled: bool){
        self.enable_detect_deadlock=enabled;
    }
}

impl ProcessControlBlock {
    /// inner_exclusive_access
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// new process from elf file
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    enable_detect_deadlock:false,

                    available_mutex:Vec::new(),
                    available_semaphore:Vec::new(),
                    need_matrix:BTreeMap::new()
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // add main thread to scheduler
        add_task(task);
        process
    }

    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        trace!("kernel: exec");
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        trace!("kernel: exec .. MemorySet::from_elf");
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        trace!("kernel: exec .. substitute memory_set");
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        trace!("kernel: exec .. push arguments on user stack");
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kstack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kstack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // add this thread to scheduler
        add_task(task);
        child
    }
    /// get pid
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// set if enabled deadlock detect
    pub fn set_deadlock_detect(&self,enabled:bool){
        let mut inner=self.inner_exclusive_access();
        inner.set_deadlock_detect(enabled);
    }

    /// get the value for the id key of tid btreemap
    pub fn get_from_btreemap(&self: BTreeMap<usize, BTreeMap<usize,u8>>, tid:usize, lock_id:usize)->Option<u8>{
        match self.find(tid){
            None=>{return None;},
            Some(tree)=>{
                match tree.find(lock_id){
                    None=>{return None;},
                    Some(v)=>{return Some(v);}
                }
            }
        }
    }

    /// add or subtract the value for the id key of the tid btreemap
    pub fn change_from_btreemap(&mut self: BTreeMap<usize, BTreeMap<usize,u8>>, 
            tid:usize, lock_id:usize, increment:u8)->Option<u8>{
        match self.find(tid){
            None=>{return None;},
            Some(tree)=>{
                match tree.find(lock_id){
                    None=>{return None;},
                    Some(v)=>{
                        v+=increment;
                        return Some(v);
                    }
                }
            }
        }
    }

    /// remove the tid btreemap from the btreemap
    pub fn remove_from_btreemap(&mut self: BTreeMap<usize, BTreeMap<usize,u8>>, tid:usize){
        self.remove(tid);
    }

    /// examine that if the deadlock would occur if lock is permitted
    /// either mutex_id or semaphore_id is valid, the invalid one should be -1 
    /// return true if deadlock is detected
    pub fn detect_deadlock(&self, tid: usize,,_mutex_id:usize, _semaphore_id:usize)->bool{
        // if both id are negative or positive
        if _mutex_id * _semaphore_id>0{
            return false;
        }
        
        let mut inner=self.inner_exclusive_access();
        let task_count=inner.thread_count();

        if _mutex_id==-1{
        // semaphore id is valid
            // write the need matrix
            let mut btreemap=
                match inner.need_matrix_for_semaphore.find(tid){
                    None=>{
                        let mut newBtree=BTreeMap::new();
                        newBtree.insert(_semaphore_id,1);
                        inner.need_matrix_for_semaphore.insert(tid,newBtree);
                        return newBtree;
                    },
                    Some(btreemap_inner)=>{
                        let result=btreemap_inner.find(_semaphore_id);
                        if result.is_none(){
                            btreemap_inner.insert(_semaphore_id,1);
                        }else{
                            let result=result.unwrap();
                            btreemap_inner.insert(_semaphore_id,result+1);
                        }
                        return btreemap_inner;
                    }
            };
            
            let detect_result=bank_algo(inner.available_semaphore.clone(),inner.need_matrix_for_semaphore.clone(),
                inner.alloc_matrix_for_semaphore.clone(),
                task_count,inner.semaphore_list.len()
            );
        
            return detect_result;

        }else{
        // mutex id is valid
            let btreemap=match inner.need_matrix_for_mutex.find(tid){
                Some(btree)=>{
                    let result=match btree.find(_mutex_id){
                        Some(r)=>{r}, None=>{0}
                    };
                    btree.insert(_mutex_id,result+1);
                    return btree;
                },
                None=>{
                    let mut btree=BTreeMap::new();
                    btree.insert(_mutex_id,1);
                    inner.need_matrix_for_mutex.insert(tid, btree);
                    return btree;
                }
            };

            let detect_result=bank_algo(inner.available_mutex.clone(),inner.need_matrix_for_mutex.clone(),
                inner.alloc_matrix_for_mutex.clone(),
                task_count,inner.mutex_list.len()
            );

            return detect_result;
        }
    }

    fn bank_algo(available: Vec<u8>,need_matrix:&BTreeMap,alloc_matrix:&BTreeMap, task_count:usize, lock_count:usize)
        ->bool{
        let mut finish=Vec::new(task_count,false);
        let check_finish=|finish_vec:&Vec<bool>|->bool{
            for i in 0..finish_vec.len(){
                if finish_vec[i]==false{
                    return true;
                }
            }
            return false;
        };

        while check_finish(&finish){
            let sources_vec=Vec::new(lock_count,0);
            let target_thread=finish.iter().enumerate().find(|(idx,val)|->{
                let need_tree=need_matrix.find(idx);
                if need_tree.is_some(){
                    let need_tree=need_tree.unwrap();
                    for i in 0..lock_count{
                        match need_tree.find(i){
                            Some(v)=>{
                                sources_vec[i]=v;
                            },
                            None=>{
                                sources_vec[i]=0;
                            }
                        }
                    }
                }
                let mut is_not_bigger=true;
                for i in 0..sources_vec.len(){
                    if sources_vec[i]>available[i]{
                        is_not_bigger=false;
                    }
                }

                return is_not_bigger && !val
            }).map(|(idx,_)| {idx});

            if target_thread.is_none(){return true;}
            let target_thread=target_thread.unwrap();
            finish[target_thread]=true;
            let need_tree=need_matrix.find(target_thread);
            let alloc_tree=alloc_matrix.find(target_thread);
            for i in 0..lock_count{
                available[i]+=(
                    match alloc_tree{
                        Some(tree)=>{
                            match tree.find(i){Some(v)=>{v},_=>{0}}
                        },
                        _=>{0}
                });
            }
            need_matrix.remove(target_thread);
            alloc_matrix.remove(target_thread);
        }

        false
    }
}
