//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, current_task_token,
        get_syscall_times, TASK_MANAGER};
use crate::mm::{VirtAddr, PhysAddr,PageTable, PageTableEntry, translated_byte_buffer,MapPermission};
use crate::timer::*;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    //first get the actual physical address
    let address= translated_byte_buffer(current_task_token(),VirtAddr::from(_ts as usize),1) as *mut TimeVal;
    let time=get_time_us();
    address.sec=time/1_000_000;
    address.usec=time % 1_000_000;
    return 0;
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request{
        //read the address of current task as u8, _id is the address
        0=>{
            //find the pageTableEntry using _id (user_address)
            if let Some(page_table_entry)= PageTable::from(current_task_token()).translate(VirtAddr::from(_id)){
                if page_table_entry.is_readable(){
                    return translated_byte_buffer(current_task_token(),VirtAddr::from(_id),1)[0][0] as isize;
                }else{
                    return -1;
                }
            }else{
                //not found
                return -1;
            }
        },

        //write the address _id of current task with _data as u8
        1=>{
            if let Some(page_table_entry)=PageTable::from(current_task_token()).translate(VirtAddr::from(_id)){
                if page_table_entry.is_writable(){
                    *(translated_byte_buffer(current_task_token(),VirtAddr::from(_id),1)[0][0])=(_data as usize);
                    return 0;
                }else{
                    return -1;
                }
            }else{
                return -1;
            }
        },

        //get the total times call of specific syscall, _id is syscallId
        2=>{
            if let Some(index)=get_syscall_index(_id){
                return get_syscall_times(index);
            }else{
                return -1;
            }
        }

        _=>{
            return -1;
        }
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    // check whether the inputs are valid
    if _port & !0x7 !=0 || _port & 0x7 ==0 {
        trace!("the permission not valid!");
        return -1;
    }
    let mut start_va=VirtAddr::from(_start);
    let end_va= VirtAddr::from(_start+_len);
    if start_va.page_offset()!=0{
        trace!("the start address does not align to the page size!");
        return -1;
    }
    
    let inner= TASK_MANAGER.inner.exclusive_access();
    let TCB= inner.tasks[inner.current_task];
    let memory_set_cur=TCB.memory_set;
    // get the page table of current task to see if the page is alloced
    loop{
        if let Some(target_pte)= page_table.translate(start_va){
            if target_pte.is_valid(){
                trace!("there is a page which has been alloced already.");
                return -1;
            }
        }
        start_va.step_one();
        if start_va > end_va{
            break;
        }
    }

    // first get the memory set of current task
    let TCB= inner.tasks[inner.current_task];
    let mut memory_set_cur= TCB.memory_set;
    
    //construct the MapPermission 
    let permission:u8 = 0;
    if _port & 0x1 !=0{
        permission |= MapPermission::R;
    }
    if _port & 0x2 !=0{
        permission |= MapPermission::W;
    }
    if _port & 0x4 !=0{
        permission |= MapPermission::X;
    }
    _port |= MapPermission::U;

    // insert the frame into the memory set
    memory_set_cur.insert_framed_area(VirtAddr::from(_start), VirtAddr::from(_start + _len), permission);
    if let Some(target_area)=memory_set_cur.areas.iter_mut()
        .find(|area| area.get_start_address()==VirtAddr::from(_start).floor()){
        target_area.map();
    }else{
        trace!("insertion of framed area has failed!");
        return -1;
    }

    return 0;
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    // shrink the mapped area
    let inner= TASK_MANAGER.inner.exclusive_access();
    let mut memory_set_cur= inner.task[inner.current_task].memory_set;
    
    let mut start_va=VirtAddr::from(_start).floor();
    let end_va=VirtAddr::from(_start+_len);
    loop{
        if let Some(target_pte)=memory_set_cur.translate(start_va){
            if !target_pte.is_valid(){
                trace!("there is a page which has not been alloced!");
                return -1;
            }
        }
        start_va.step_one();
        if start_va>end_va{
            break;
        }
    }
    loop{
        start_va= VirtAddr::from(_start).floor();
        memory_set_cur.empty_one_pte(start_va);
        start_va.step_one();
        if start_va>end_va{
            break;
        }
    }
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
