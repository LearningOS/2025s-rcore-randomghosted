use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
use crate::BLOCK_SZ;
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<(usize,u32)> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some((i,dirent.inode_id() as u32));
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|(_idx,inode_id)| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Find inode under current inode by name with the index in the directory returned
    pub fn find_and_get_idx(&self, name: &str) -> Option<(usize,Arc<Inode>)> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|(idx,inode_id)| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                (idx,Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                )))
            })
        })
    }

    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }

    /// Decrease the size of a disk Inode
    /// before decreasing the size, work must be done to make sure that the shrinked space is
    /// translate
    fn decrease_size(&self, new_size:u32, disk_inode:&mut DiskInode, fs:&mut MutexGuard<EasyFileSystem>){
        if new_size>=self.size{return;}
        let dealloc_data_block_id=disk_inode.decrease_size(new_size,&self.block_device);
        dealloc_data_block_id.iter().enumerate().for_each(|(_,block_id)|{
            fs.dealloc_data(*block_id as u32);
        })
    }

    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }

    /// Clear the disk inode block for the file, and itself should be destruct
    pub fn clear_inode(&self){
        self.fs.lock().dealloc_inode(self.block_id as usize, self.block_offset as usize);
    }
}

/// impl the get stat function
impl Inode{
    /// is file or not
    pub fn is_file(&self)->bool{
        self.read_disk_inode(|disk_inode:&DiskInode|->bool{
            disk_inode.is_file()
        })
    }

    /// is directory or not
    pub fn is_directory(&self)->bool{
        self.read_disk_inode(|disk_inode:&DiskInode|->bool{
            disk_inode.is_dir()
        })
    }

    /// get num of hard links
    pub fn get_num_of_links(&self)->u32{
        self.read_disk_inode(|disk_inode:&DiskInode|->u32{
            disk_inode.nlink
        })
    }
}

/// other helper function
impl Inode{
    /// get the inode_id from block_id and block_offset
    pub fn get_inode_id(&self)->u32{
        self.fs.lock().get_inode_id(self.block_id as usize, self.block_offset as usize)
    }

    /// append an file entry in the directory, but not alloc any inode
    fn append_file_entry(&self, file_entry:DirEntry){
        let mut fs=self.fs.lock();
        self.modify_disk_inode(|disk_inode:&mut DiskInode| {
            // append file in the dirent
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, disk_inode, &mut fs);
            disk_inode.write_at(
                file_count * DIRENT_SZ,
                file_entry.as_bytes(),
                &self.block_device,
            );
        });
    }

    /// delete the file, with dealloc data, inode and file entry
    fn delete_file(&mut self, name:&str)->isize{
        if !self.is_directory(){return -1;}
        let result=self.find_and_get_idx(name);
        if result.is_none(){return -1;}
        let idx=result.0;
        let inode=result.1;

        // clear the data and inode
        self.clear();
        self.clear_inode();

        let current_size=self.size;
        let mut after_size=current_size-DIRENT_SZ;
        inode.modify_disk_inode(|disk_inode:&mut DiskInode|{
            let buf=[0usize;DIRENT_SZ];
            self.read_at(current_size-DIRENT_SZ,&mut buf);
            inode.write_at(idx*DIRENT_SZ,&buf);
        });

        let fs=inode.fs.lock();
        inode.modify_disk_inode(|disk_inode:&mut DiskInode|{
            inode.decrease_size(after_size,disk_inode,fs);
        })
        0
    }
}

/// link the new file path to the old file path, hard link
pub fn linkat(old_path:&str, new_path:&str, where_to_link:&Arc<Inode>)->isize{
    if !where_to_link.is_directory(){return -1;}
    let old_file_inode=where_to_link.find(old_path);
    let new_file_inode=where_to_link.find(new_path);
    if old_file_inode.is_none() || new_file_inode.is_some(){
        return -1;
    }

    let old_file_inode=old_file_inode.unwrap();
    let old_file_inode_id=old_file_inode.get_inode_id();
    let dirent=DirEntry::new(new_path,old_file_inode_id);
 
    let add_link_result=old_file_inode.modify_disk_inode(|disk_inode:&mut DiskInode|->isize{
        disk_inode.add_link() 
    });
    if add_link_result==-1{
        return -1;
    }else{
        where_to_link.append_file_entry(dirent);
    }
    
    0
}

/// unlink the file path 
pub fn unlinkat(path:&str,where_to_unlink:&Arc<Inode>)->isize{
    if !where_to_unlink.is_directory(){return -1;}
    let file_inode=where_to_unlink.find(path);
    if file_inode.is_none(){return -1;}
    let file_inode=file_inode.unwrap();

    let current_link=file_inode.get_num_of_links();
    assert(current_link>0);
    if current_link>1{
        file_inode.modify_disk_inode(|disk_inode:&mut DiskInode|{
            disk_inode.subtract_link();
        })
    }else{
        return where_to_unlink.delete_file(path);
    }
    0
}
