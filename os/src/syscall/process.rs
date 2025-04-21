//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next, get_syscall_times},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// TODO: implement the syscall
pub fn sys_trace(trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request{
        //read the _id as u8 address of current task
        0=>{
//            let current_task_address=APP_BASE_ADDRESS + get_current_task_id() * APP_SIZE_LIMIT;
//            let target_address=current_task_address + _id;
            let target_address=_id;
            return unsafe{(core::slice::from_raw_parts(target_address as *const u8, 1)[0]) as usize as isize};
        },
        //write the _id as u8 address of current task
        1=>{
//            let current_task_address=APP_BASE_ADDRESS + get_current_task_id() * APP_SIZE_LIMIT;
//            let target_address=current_task_address + _id;
            let target_address=_id;
            unsafe {(target_address as *mut u8).write_volatile(_data as u8);}
            return 0;
        },
        2=>{
            if let Some(syscall_times)=get_syscall_times(_id){
                return syscall_times as isize;
            }else{
                return -1;
            }
        },
        _=>{
            return -1;
        }
    } 
}
