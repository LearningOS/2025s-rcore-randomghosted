//! Loading user applications into memory

/// Get the total number of applications.
use alloc::vec::Vec;
use crate::fs::{OpenFlags,open_file};

#[allow(unused)]
///get app data from name
pub fn get_app_data_by_name(name: &str) -> Option<Vec<u8>> {
    open_file(name,OpenFlags::RDONLY).map(|inode|{
        inode.read_all()
    })
}
