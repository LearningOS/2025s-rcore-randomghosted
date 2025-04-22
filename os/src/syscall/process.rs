//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, current_task_token};
use crate::mm::{VirtAddr, PhysAddr,PageTable, PageTableEntry, translated_byte_buffer};

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
    -1
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request{
        //read the address of current task as u8, _id is the address
        0=>{
            //find the pageTableEntry using _id (user_address)
            if let Some(page_table_entry)= translate(VirtAddr::from(_id)){
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
            if let Some(page_table_entry)=translate(VirtAddr::from(_id)){
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
            
        }

        _=>{
            return -1;
        }
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    -1
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
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
