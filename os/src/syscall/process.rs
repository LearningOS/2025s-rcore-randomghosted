//! Process management syscalls
//!
//use alloc::sync::Arc;
use core::mem::size_of;

use crate::{
    fs::{open_file, OpenFlags},
    mm::{translated_refmut, translated_str},
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str, translated_byte_buffer, VirtAddr, MapPermission, PageTable, VPNRange},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next
    },
    timer::*,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
       // let mut found_pid=0;
           
            //let child=inner.children.get(idx);
            //println!("count: {}", Arc::strong_count(&child.unwrap()));
            let child = inner.children.remove(idx);
            let found_pid = child.getpid();
            // ++++ temporarily access child PCB exclusively
            let exit_code = child.inner_exclusive_access().exit_code;
            // ++++ release child PCB
            *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        
        // confirm that child will be deallocated after being removed from children list
        // assert_eq!(Arc::strong_count(&child), 1);
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let mut timeval_slice=translated_byte_buffer(current_user_token(),_ts as usize as *const u8,size_of::<TimeVal>());
    let current_time=get_time();
    let mut timeval_current=TimeVal{sec:current_time/1_000_000, usec:current_time%1_000_000};
    unsafe{
       let timeval_current_slice=core::slice::from_raw_parts(&mut timeval_current as *mut TimeVal as *mut u8, size_of::<TimeVal>());
        let mut count=0;
        for i in 0..timeval_slice.len(){
            let length=timeval_slice[i].len();
            timeval_slice[i].copy_from_slice(&timeval_current_slice[count..count+length]);
            count+=timeval_slice[i].len();
        }
    }
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    // check input validation
    if VirtAddr::from(_start).page_offset()!=0 || _port==0 || _port & !0x7 !=0 {
        return -1;
    }
    if _len==0{
        return 0;
    }

    // check if there is already map area in current task
    let target_task=current_task().unwrap();
    let current_task_memory_set=target_task.get_memory_set();

    let check_point_in_range=|left_bound:usize, right_bound:usize, target:usize|->bool{
        return target>=left_bound && target<right_bound;
    };

    let _end=VirtAddr::from(_start+_len).ceil().0;
    let _start=VirtAddr::from(_start).floor().0;
    
    if current_task_memory_set.areas.iter().any(|area| 
        check_point_in_range(_start,_end,area.vpn_range.get_start().0) 
        || check_point_in_range(_start,_end,area.vpn_range.get_end().0) 
        || check_point_in_range(area.vpn_range.get_start().0, area.vpn_range.get_end().0, _start) 
        || check_point_in_range(area.vpn_range.get_start().0,area.vpn_range.get_end().0,_end)){
        return -1;
    }
   
    let mut permission= MapPermission::U;
    if _port & 0x1 !=0{ permission |= MapPermission::R; }
    if _port & 0x2 !=0{ permission |= MapPermission::W; }
    if _port & 0x4 !=0{ permission |= MapPermission::X; }

    current_task_memory_set.insert_framed_area(VirtAddr::from(_start),VirtAddr::from(_end), permission);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if VirtAddr::from(_start).page_offset()!=0{
        return -1;
    }
    if _len==0{
        return 0;
    }
    let mut page_table=PageTable::from_token(current_user_token());
    let unmap_vpn_range=VPNRange::new(VirtAddr::from(_start).floor(),VirtAddr::from(_start+_len).ceil());
    if unmap_vpn_range.into_iter().enumerate().any(|(_,vpn)| page_table.translate(vpn).is_none()){
        return -1;
    }
    let _= unmap_vpn_range.into_iter().enumerate().map(|(_,vpn)| page_table.unmap(vpn));
    0
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    let task=current_task().unwrap();
    if let Some(child)=task.spawn(_path){
        let childpid=child.getpid();
        add_task(child);
        childpid as isize
    }else{
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    let task=current_task().unwrap();
    let result=task.set_priority(_prio);
    result
}




