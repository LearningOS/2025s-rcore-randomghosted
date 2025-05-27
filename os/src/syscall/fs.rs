//! File and filesystem-related syscalls
use crate::fs::{open_file, OpenFlags, Stat, OSInode, StatMode, ROOT_INODE};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};
use alloc::sync::Arc;
use easy_fs::{linkat,unlinkat};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    let task=current_task().unwrap();
    let task_inner=task.inner_exclusive_access();

    if _fd>=task_inner.fd_table.len() || task_inner.fd_table[_fd].is_none(){
        return -1;
    }

    let file_inode=task_inner.fd_table[_fd].clone().unwrap();
    drop(task_inner);

    // check if the fd is corresponding to the disk file or not
    if !file_inode.is_disk_file(){
        return -1;
    }

    // transform to the OSInode
    let file_inode= unsafe{ Arc::from_raw(Arc::into_raw(file_inode) as *const _ as *const OSInode) };

    let mut stat=Stat::default();
    stat.ino=file_inode.get_inode_id() as u64;
    if file_inode.is_file(){
        stat.mode=StatMode::FILE;
    }else if file_inode.is_directory(){
        stat.mode=StatMode::DIR;
    }else{
        stat.mode=StatMode::NULL;
    }
    stat.nlink=file_inode.get_num_of_hard_links();

    let stat_slice=unsafe {core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>()) };
    let mut target_stat_slice=translated_byte_buffer(current_user_token(),_st as *mut _ as *mut u8, core::mem::size_of::<Stat>());
    let mut count=0;
    for i in 0..target_stat_slice.len(){
        let len_=target_stat_slice[i].len();
        target_stat_slice[i].copy_from_slice(&stat_slice[count..count+len_]);
        count+=len_;
    }

    0
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    let old_file_name=translated_str(current_user_token(),_old_name);
    let new_file_name=translated_str(current_user_token(),_new_name);

    let result=linkat(&old_file_name,&new_file_name,&ROOT_INODE);
    result
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    let file_name=translated_str(current_user_token(),_name);
    let result=unlinkat(&file_name,&ROOT_INODE);

    result as isize
}
