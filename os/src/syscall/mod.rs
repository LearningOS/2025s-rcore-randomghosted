//! Implementation of syscalls
//!
//! The single entry point to all system calls, [`syscall()`], is called
//! whenever userspace wishes to perform a system call using the `ecall`
//! instruction. In this case, the processor raises an 'Environment call from
//! U-mode' exception, which is handled as one of the cases in
//! [`crate::trap::trap_handler`].
//!
//! For clarity, each single syscall is implemented as its own function, named
//! `sys_` then the name of the syscall. You can find functions like this in
//! submodules, and you should also implement syscalls this way.

/// write syscall
const SYSCALL_WRITE: usize = 64;
/// exit syscall
const SYSCALL_EXIT: usize = 93;
/// yield syscall
const SYSCALL_YIELD: usize = 124;
/// gettime syscall
const SYSCALL_GET_TIME: usize = 169;
/// trace syscall
const SYSCALL_TRACE: usize = 410;
/// syscall id list
const SYSCALL_ID_LIST: [usize;5] =[64,93,124,169,410];


mod fs;
mod process;

use fs::*;
use process::*;
use crate::task::add_syscall_times_once;

/// handle syscall exception with `syscall_id` and other arguments
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    add_syscall_times_once(syscall_id);
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_TRACE => sys_trace(args[0], args[1], args[2]),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}

///help to get syscall id index in the syscall id list, using binary search
pub fn get_syscall_id_index(syscall_id:usize)->Option<usize>{
    let mut left=0; let mut right=SYSCALL_ID_LIST.len()-1;
    let mut result: usize=0;
    let mut enter_flag=false;
    while left<=right{
        let mid=(left+right)/2;
        if SYSCALL_ID_LIST[mid]>syscall_id{
            right=mid-1;
        }else if SYSCALL_ID_LIST[mid]==syscall_id{
            enter_flag=true;
            result=mid;
            break;
        }else{
            left=mid+1;
        }
    }
    if !enter_flag{
        None
    }else{
        Some(result as usize)
    }
}
