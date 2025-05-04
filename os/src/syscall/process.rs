//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, current_user_token,
        get_syscall_times, current_user_memory_set};
use crate::mm::{VirtPageNum,VirtAddr,PageTable, translated_byte_buffer,MapPermission};
use crate::mm::{VA_WIDTH_SV39};
use crate::timer::*;
use crate::config::PAGE_SIZE;
use super::get_syscall_index;
use core::mem::size_of;

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
    let mut timeval_slice=translated_byte_buffer(current_user_token(),_ts as *mut u8, size_of::<TimeVal>());
    let time=get_time_us();
    let timeval=TimeVal{
        sec:time/1_000_000, usec:time % 1_000_000
    };
    
    let timeval_new_slice=unsafe{core::slice::from_raw_parts(&timeval as *const TimeVal as *const u8,size_of::<TimeVal>())};

    let mut count=0;
    for i in 0..timeval_slice.len(){
        for j in 0..timeval_slice[i].len(){
            timeval_slice[i][j]=timeval_new_slice[count];
            count+=1;
        }
    }
    return 0;
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request{
        //read the address of current task as u8, _id is the address
        0=>{
            if _id>(1<<VA_WIDTH_SV39){
                    return -1;
            }

            if let Some(page_table_entry)=PageTable::from_token(current_user_token()).translate(VirtAddr::from(_id).into()){
                if page_table_entry.readable(){
                    return translated_byte_buffer(current_user_token(),_id as *const u8,1)[0][0] as isize;
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
            if _id > (1<<VA_WIDTH_SV39){
                return -1;
            }

            if let Some(page_table_entry)=PageTable::from_token(current_user_token()).translate(VirtAddr::from(_id).into()){
                if page_table_entry.writable(){
                    translated_byte_buffer(current_user_token(),_id as *const u8,1)[0][0]=_data as u8;
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
                if let Some(result)= get_syscall_times(index){
                    return result as isize;
                }else{
                    return -1;
                }
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
    
    let page_table_cur=PageTable::from_token(current_user_token());
    // get the page table of current task to see if the page is alloced
    loop{
        if let Some(target_pte)= page_table_cur.translate(start_va.into()){
            if target_pte.is_valid(){
                trace!("there is a page which has been alloced already.");
                return -1;
            }
        }
        start_va=VirtAddr::from(start_va.0+PAGE_SIZE);
        if start_va > end_va{
            break;
        }
    }
    
    //construct the MapPermission 
    let mut permission= MapPermission::U;
    if _port & 0x1 !=0{
        permission |= MapPermission::R;
    }
    if _port & 0x2 !=0{
        permission |= MapPermission::W;
    }
    if _port & 0x4 !=0{
        permission |= MapPermission::X;
    }

    // insert the frame into the memory set
    current_user_memory_set().insert_framed_area(VirtAddr::from(_start), VirtAddr::from(_start + _len), permission);
    return 0;
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    
    if VirtAddr::from(_start).page_offset()!=0{
        return -1;
    }
    
    // shrink the mapped area
    let mut page_table_cur=PageTable::from_token(current_user_token());

    let mut start_va=VirtAddr::from(_start).floor();
    let end_va=VirtAddr::from(_start+_len);
    loop{
        if let Some(target_pte)=page_table_cur.translate(start_va.into()){
            if !target_pte.is_valid(){
                println!("{}",start_va.0);
                println!("there is a page which has not been alloced!");
                return -1;
            }
        }
        start_va=VirtPageNum::from(start_va.0+1);
        if VirtAddr::from(start_va)>=end_va{
            break;
        }
    }
    start_va=VirtAddr::from(_start).floor();
    loop{
        page_table_cur.unmap(start_va);
        start_va=VirtPageNum::from(start_va.0+1);
        if VirtAddr::from(start_va)>=end_va{
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
