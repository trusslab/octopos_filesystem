use std::cell::RefCell;  
use std::collections::LinkedList;  
use std::rc::Rc;  
use std::path::Path;
use std::fs::File as FsFile;  
use std::io::{Read, Write};  
use std::borrow::BorrowMut;

// Constants  
const MAX_NUM_FD: usize = 64;  
pub const FILE_OPEN_MODE: u32 = 0;  
pub const FILE_OPEN_CREATE_MODE: u32 = 1;  
pub const STORAGE_BLOCK_SIZE: usize = 512;  
const DIR_DATA_NUM_BLOCKS: usize = 2;  
const DIR_DATA_SIZE: usize = STORAGE_BLOCK_SIZE * DIR_DATA_NUM_BLOCKS;  
const MAX_FILENAME_SIZE: usize = 256;  
const ERR_INVALID: i32 = -2;  
const ERR_EXIST: i32 = -5;  
const ERR_MEMORY: i32 = -6;  
const ERR_FOUND: i32 = -7;  
  
// File structure  
#[derive(Debug)]  
struct File {  
    filename: String,  
    start_block: u32,  
    num_blocks: u32,  
    size: u32,  
    dir_data_off: usize,  
    opened: bool,  
}  
  
// Helper function to create a default array of Option<Rc<RefCell<File>>> with None  
fn default_file_array() -> [Option<Rc<RefCell<File>>>; MAX_NUM_FD] {  
    std::array::from_fn(|_| None)  
}   

// Global mutable data structures with RefCell for interior mutability  
thread_local! {  
    static PARTITION_NUM_BLOCKS: RefCell<u32> = RefCell::new(0);  
    static FD_BITMAP: RefCell<[u8; MAX_NUM_FD / 8]> = RefCell::new([0; MAX_NUM_FD / 8]);  
    static FILE_ARRAY: RefCell<[Option<Rc<RefCell<File>>>; MAX_NUM_FD]> = RefCell::new(default_file_array());    
    static FILE_LIST: RefCell<LinkedList<Rc<RefCell<File>>>> = RefCell::new(LinkedList::new());  
    static DIR_DATA: RefCell<[u8; DIR_DATA_SIZE]> = RefCell::new([0; DIR_DATA_SIZE]);  
    static DIR_DATA_PTR: RefCell<usize> = RefCell::new(0);  
}  
  
// Function prototypes  
pub fn file_system_open_file(filename: &str, mode: u32) -> Result<u32, i32> {  
    if mode != FILE_OPEN_MODE && mode != FILE_OPEN_CREATE_MODE {  
        eprintln!("Error: invalid mode for opening a file");  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    let mut file = None;  
  
    // Search for the file in the file list  
    FILE_LIST.with(|file_list| {  
        let file_list = file_list.borrow();  
        for node in file_list.iter() {  
            let node_file = node.borrow();  
            if node_file.filename == filename {  
                if node_file.opened {  
                    return; // Error: file already opened  
                }  
                file = Some(Rc::clone(node));  
                break;  
            }  
        }  
    });  
  
    // If the file is not found and mode is FILE_OPEN_CREATE_MODE, create the file  
    if file.is_none() && mode == FILE_OPEN_CREATE_MODE {  
        let new_file = Rc::new(RefCell::new(File {  
            filename: filename.to_string(),  
            start_block: 0,  
            num_blocks: 0,  
            size: 0,  
            dir_data_off: 0,  
            opened: false,  
        }));  
  
        {  
            // Explicitly scope the mutable borrow to ensure it is released after use  
            let mut new_file_borrow = RefCell::borrow_mut(&new_file);  
            let ret = add_file_to_directory(&mut new_file_borrow);  
            if ret.is_err() {  
                release_file_blocks(&new_file_borrow);  
                return Ok(0); // Return 0 to mirror the original C code behavior  
            }  
        }  
  
        if add_file_to_list(Rc::clone(&new_file)).is_err() {  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
  
        file = Some(new_file);  
    }  
  
    // Proceed to get an unused file descriptor if the file is found or successfully created  
    if let Some(file_rc) = file {  
        let fd = match get_unused_fd() {  
            Ok(fd) => fd,  
            Err(_) => return Ok(0), // Return 0 to mirror the original C code behavior  
        };  
  
        if fd == 0 || fd >= MAX_NUM_FD as u32 {  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
  
        let result = FILE_ARRAY.with(|file_array| {  
            let mut file_array = file_array.borrow_mut();  
            if file_array[fd as usize].is_some() {  
                return Err(()); // Return Err to indicate failure  
            }  
            file_array[fd as usize] = Some(Rc::clone(&file_rc));  
            Ok(())  
        });  
  
        if result.is_err() {  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
  
        RefCell::borrow_mut(&file_rc).opened = true;  
        return Ok(fd);  
    }  
  
    // Error: file not found or couldn't be created  
    Ok(0) // Return 0 to mirror the original C code behavior  
}  
  
pub fn file_system_write_to_file(fd: u32, data: &[u8], size: u32, offset: u32) -> Result<u32, i32> {  
    if fd == 0 || fd as usize >= MAX_NUM_FD {  
        eprintln!("Error: file_system_write_to_file: fd is 0 or too large ({})", fd);  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    let file_option = FILE_ARRAY.with(|file_array| file_array.borrow()[fd as usize].clone());  
    let file_rc = match file_option {  
        Some(file_rc) => file_rc,  
        None => {  
            eprintln!("Error: file_system_write_to_file: invalid fd");  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
    };  
  
    let mut file = RefCell::borrow_mut(&file_rc);  
    if !file.opened {  
        eprintln!("Error: file_system_write_to_file: file not opened!");  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    if file.size < (offset + size) {  
        if offset > file.size {  
            eprintln!(  
                "Error: file_system_write_to_file: invalid offset (offset = {}, file.size = {})",  
                offset, file.size  
            );  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
        // Try to expand the file size  
        drop(file);
        expand_file_size(fd as usize, offset + size)?;  //MISTAKE
    }  
    let mut file = RefCell::borrow_mut(&file_rc);  
  
    if offset >= file.size {  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    // Partial write  
    let size = if file.size < (offset + size) {  
        file.size - offset  
    } else {  
        size  
    };  
  
    let mut block_num = offset / STORAGE_BLOCK_SIZE as u32;  
    let mut block_offset = offset % STORAGE_BLOCK_SIZE as u32;  
    let mut written_size = 0;  
    let mut next_write_size = STORAGE_BLOCK_SIZE as u32 - block_offset;  
    if next_write_size > size {  
        next_write_size = size;  
    }  
  
    while written_size < size {  
        let ret = write_to_block(  
            &data[written_size as usize..(written_size + next_write_size) as usize],  
            file.start_block + block_num,  
            block_offset,  
            next_write_size,  
        )?;  //kinda a MISTAKE
  
        if ret != next_write_size { 
            written_size += ret;  
            break;  
        }  
        written_size += next_write_size;  
        block_num += 1;  
        block_offset = 0;  
        if (size - written_size) >= STORAGE_BLOCK_SIZE as u32 {  
            next_write_size = STORAGE_BLOCK_SIZE as u32;  
        } else {  
            next_write_size = size - written_size;  
        }  
    }  
  
    Ok(written_size)  
}  

  
pub fn file_system_read_from_file(fd: u32, data: &mut [u8], size: u32, offset: u32) -> Result<u32, i32> {  
    if fd == 0 || fd as usize >= MAX_NUM_FD {  
        eprintln!("Error: file_system_read_from_file: fd is 0 or too large ({})", fd);  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    let file_option = FILE_ARRAY.with(|file_array| file_array.borrow()[fd as usize].clone());  
    let file_rc = match file_option {  
        Some(file_rc) => file_rc,  
        None => {  
            eprintln!("Error: file_system_read_from_file: invalid fd");  
            return Ok(0); // Return 0 to mirror the original C code behavior  
        }  
    };  
  
    let file = file_rc.borrow();  
    if !file.opened {  
        eprintln!("Error: file_system_read_from_file: file not opened!");  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    if offset >= file.size {  
        return Ok(0); // Return 0 to mirror the original C code behavior  
    }  
  
    // Partial read  
    let size = if file.size < (offset + size) {  
        file.size - offset  
    } else {  
        size  
    };  
  
    let mut block_num = offset / STORAGE_BLOCK_SIZE as u32;  
    let mut block_offset = offset % STORAGE_BLOCK_SIZE as u32;  
    let mut read_size = 0;  
    let mut next_read_size = STORAGE_BLOCK_SIZE as u32 - block_offset;  
    if next_read_size > size {  
        next_read_size = size;  
    }  
  
    while read_size < size {  
        let ret = read_from_block(  
            &mut data[read_size as usize..(read_size + next_read_size) as usize],  
            file.start_block + block_num,  
            block_offset,  
            next_read_size,  
        )?;  // Maybe MISTAKE
  
        if ret != next_read_size {  
            read_size += ret;  
            break;  
        }  
        read_size += next_read_size;  
        block_num += 1;  
        block_offset = 0;  
        if (size - read_size) >= STORAGE_BLOCK_SIZE as u32 {  
            next_read_size = STORAGE_BLOCK_SIZE as u32;  
        } else {  
            next_read_size = size - read_size;  
        }  
    }  
  
    Ok(read_size)  
}  

  
pub fn file_system_close_file(fd: u32) -> Result<(), i32> {  
    if fd == 0 || fd as usize >= MAX_NUM_FD {  
        eprintln!("Error: file_system_close_file: fd is 0 or too large ({})", fd);  
        return Err(ERR_INVALID); // Return ERR_INVALID to mirror the original C code behavior  
    }  
  
    let file_option = FILE_ARRAY.with(|file_array| file_array.borrow()[fd as usize].clone());  
    let file_rc = match file_option {  
        Some(file_rc) => file_rc,  
        None => {  
            eprintln!("Error: file_system_close_file: invalid fd");  
            return Err(ERR_INVALID); // Return ERR_INVALID to mirror the original C code behavior  
        }  
    };  
  
    let mut file = RefCell::borrow_mut(&file_rc);  
    if !file.opened {  
        eprintln!("Error: file_system_close_file: file not opened!");  
        return Err(ERR_INVALID); // Return ERR_INVALID to mirror the original C code behavior  
    }  
  
    file.opened = false;  
    FILE_ARRAY.with(|file_array| file_array.borrow_mut()[fd as usize] = None);  
    mark_fd_as_unused(fd);  
  
    Ok(())  
}  
  
pub fn initialize_file_system(partition_num_blocks: u32) {  
    FILE_LIST.with(|file_list| {  
        let mut file_list = file_list.borrow_mut();  
        file_list.clear();  
    });  
  
    DIR_DATA_PTR.with(|dir_data_ptr| {  
        *dir_data_ptr.borrow_mut() = 0;  
    });  
  
    PARTITION_NUM_BLOCKS.with(|partition| {  
        *partition.borrow_mut() = 0;  
    });  
  
    // Initialize fd bitmap  
    if MAX_NUM_FD % 8 != 0 {  
        eprintln!("Error: initialize_file_system: MAX_NUM_FD must be divisible by 8");  
        std::process::exit(-1);  
    }  
  
    FD_BITMAP.with(|fd_bitmap| {  
        let mut fd_bitmap = fd_bitmap.borrow_mut();  
        fd_bitmap[0] = 0x01; // fd 0 is error  
        for i in 1..(MAX_NUM_FD / 8) {  
            fd_bitmap[i] = 0;  
        }  
    });  
  
    PARTITION_NUM_BLOCKS.with(|partition| {  
        *partition.borrow_mut() = partition_num_blocks;  
    });  
  
    // Read the directory  
    read_dir_data_from_storage();  
  
    DIR_DATA.with(|dir_data_o| {  
        let mut dir_data = dir_data_o.borrow();  
        if dir_data[0] == b'$' && dir_data[1] == b'%' && dir_data[2] == b'^' && dir_data[3] == b'&' {  
            // Retrieve file info  
            let num_files = u16::from_le_bytes([dir_data[4], dir_data[5]]);  
            DIR_DATA_PTR.with(|dir_data_ptr| {  
                *dir_data_ptr.borrow_mut() = 6;  
            });  
  
            for _ in 0..num_files {  
                let mut dir_data_off = 0;  
                let filename_size;  
                let filename;  
                let mut file_rc;  
  
                DIR_DATA_PTR.with(|dir_data_ptr| {  
                    dir_data_off = *dir_data_ptr.borrow();  
                });  
  
                if dir_data_off + 2 > DIR_DATA_SIZE {  
                    break;  
                }  
  
                filename_size = u16::from_le_bytes([dir_data[dir_data_off], dir_data[dir_data_off + 1]]) as usize;  
  
                if dir_data_off + filename_size + 15 > DIR_DATA_SIZE {  
                    break;  
                }  
  
                DIR_DATA_PTR.with(|dir_data_ptr| {  
                    *dir_data_ptr.borrow_mut() += 2;  
                });  
  
                if filename_size > MAX_FILENAME_SIZE {  
                    break;  
                }  
  
                filename = String::from_utf8_lossy(&dir_data[dir_data_off + 2..dir_data_off + 2 + filename_size]).to_string();  
  
                DIR_DATA_PTR.with(|dir_data_ptr| {  
                    *dir_data_ptr.borrow_mut() += filename_size + 1;  
                });  
  
                file_rc = Rc::new(RefCell::new(File {  
                    filename,  
                    start_block: u32::from_le_bytes([dir_data[dir_data_off + 2 + filename_size + 1], dir_data[dir_data_off + 2 + filename_size + 2], dir_data[dir_data_off + 2 + filename_size + 3], dir_data[dir_data_off + 2 + filename_size + 4]]),  
                    num_blocks: u32::from_le_bytes([dir_data[dir_data_off + 2 + filename_size + 5], dir_data[dir_data_off + 2 + filename_size + 6], dir_data[dir_data_off + 2 + filename_size + 7], dir_data[dir_data_off + 2 + filename_size + 8]]),  
                    size: u32::from_le_bytes([dir_data[dir_data_off + 2 + filename_size + 9], dir_data[dir_data_off + 2 + filename_size + 10], dir_data[dir_data_off + 2 + filename_size + 11], dir_data[dir_data_off + 2 + filename_size + 12]]),  
                    dir_data_off,  
                    opened: false,  
                }));  
  
                add_file_to_list(file_rc).unwrap();  
            }  
        } else {  
            // Initialize signature  
            drop(dir_data);
            let mut dir_data = RefCell::borrow_mut(&dir_data_o);  
            dir_data[0] = b'$';  
            dir_data[1] = b'%';  
            dir_data[2] = b'^';  
            dir_data[3] = b'&';  
            dir_data[4] = 0;  
            dir_data[5] = 0;  
            DIR_DATA_PTR.with(|dir_data_ptr| {  
                *dir_data_ptr.borrow_mut() = 6;  
            });  

            drop(dir_data);
  
            // Update the directory in storage  
            flush_dir_data_to_storage();  
        }  
    });  
  
    FILE_ARRAY.with(|file_array| {  
        let mut file_array = file_array.borrow_mut();  
        for i in 0..MAX_NUM_FD {  
            file_array[i] = None;  
        }  
    });  
}  

  
pub fn close_file_system() {  
    flush_dir_data_to_storage();
}  
  
fn get_unused_fd() -> Result<u32, i32> {  
    FD_BITMAP.with(|fd_bitmap| {  
        let mut fd_bitmap = fd_bitmap.borrow_mut();  
        for i in 0..(MAX_NUM_FD / 8) {  
            if fd_bitmap[i] == 0xFF {  
                continue;  
            }  
            let mut mask = 0b00000001;  
            for j in 0..8 {  
                if (fd_bitmap[i] | !mask) != 0xFF {  
                    fd_bitmap[i] |= mask;  
                    return Ok((i * 8 + j) as u32 + 1);  
                }  
                mask <<= 1;  
            }  
        }  
        Err(ERR_EXIST)  
    })  
}  

  
fn mark_fd_as_unused(fd: u32) {  
    // Ensure fd is within valid range  
    if fd == 0 || fd > MAX_NUM_FD as u32 {  
        eprintln!("Error: mark_fd_as_unused: invalid fd {}", fd);  
        return;  
    }  
  
    let fd = fd - 1; // Adjust fd to be zero-based  
    let byte_off = fd as usize / 8;  
    let bit_off = fd as usize % 8;  
    let mask = !(1 << bit_off);  
  
    FD_BITMAP.with(|fd_bitmap| {  
        let mut fd_bitmap = fd_bitmap.borrow_mut();  
        fd_bitmap[byte_off] &= mask;  
    });  
}  
  
fn add_file_to_list(file: Rc<RefCell<File>>) -> Result<(), i32> {  
    FILE_LIST.with(|file_list| {  
        let mut file_list = file_list.borrow_mut();  
        file_list.push_back(file);  
        Ok(())  
    })  
} 
  
// Function to write blocks of data to files  
fn write_blocks(data: &[u8], start_block: u32, num_blocks: u32) -> Result<u32, i32> {  
    let mut written: u32 = 0;  
  
    for i in 0..num_blocks {  
        let block_num = start_block + i;  
        let block_name = format!("block{}.txt", block_num);  
        let path = Path::new(&block_name);  
  
        let mut file = match FsFile::create(&path) {  
            Ok(f) => f,  
            Err(_) => {  
                eprintln!("Error: Failed to open block file");  
                return Ok(written);  
            }  
        };  
  
        let start_index = (i as usize) * STORAGE_BLOCK_SIZE;  
        let end_index = start_index + STORAGE_BLOCK_SIZE;  
  
        let ret = match file.write_all(&data[start_index..end_index]) {  
            Ok(_) => STORAGE_BLOCK_SIZE,  
            Err(_) => {  
                eprintln!("Error: Failed to write to block file");  
                return Ok(written);  
            }  
        };  
  
        written += ret as u32;  
    }  
  
    Ok(written)  
}  
  
// Function to read blocks of data from files  
fn read_blocks(data: &mut [u8], start_block: u32, num_blocks: u32) -> Result<u32, i32> {  
    let mut read: u32 = 0;  
  
    for i in 0..num_blocks {  
        let block_num = start_block + i;  
        let block_name = format!("block{}.txt", block_num);  
        let path = Path::new(&block_name);  
  
        let mut file = match FsFile::open(&path) {  
            Ok(f) => f,  
            Err(_) => {  
                // Create a zeroed block and write it  
                let zero_buf = vec![0u8; STORAGE_BLOCK_SIZE];  
                write_blocks(&zero_buf, block_num, 1)?;  
  
                // Try opening the file again  
                match FsFile::open(&path) {  
                    Ok(f) => f,  
                    Err(_) => {  
                        eprintln!("Error: Failed to open block file {}", block_name);  
                        return Ok(read);  
                    }  
                }  
            }  
        };  
  
        let start_index = (i as usize) * STORAGE_BLOCK_SIZE;  
        let end_index = start_index + STORAGE_BLOCK_SIZE;  
  
        match file.read_exact(&mut data[start_index..end_index]) {  
            Ok(_) => {  
                read += STORAGE_BLOCK_SIZE as u32;  
            }  
            Err(_) => {  
                eprintln!("Error: Failed to read block file {}", block_name);  
                return Ok(read);  
            }  
        };  
    }  
  
    Ok(read)  
}  
  
fn read_from_block(data: &mut [u8], block_num: u32, block_offset: u32, read_size: u32) -> Result<u32, i32> {  
    let mut buf = vec![0u8; STORAGE_BLOCK_SIZE];  
  
    // Check if the read operation would overflow the block size  
    if block_offset + read_size > STORAGE_BLOCK_SIZE as u32 {  
        return Ok(0);  
    }  
  
    // Read the block into the buffer  
    let ret = read_blocks(&mut buf, block_num, 1)?;  
    if ret != STORAGE_BLOCK_SIZE as u32 {  
        return Ok(0);  
    }  
  
    // Perform the copy from buf to data  
    data[..read_size as usize].copy_from_slice(&buf[block_offset as usize..(block_offset + read_size) as usize]);  
  
    Ok(read_size)  
}  
  
fn write_to_block(data: &[u8], block_num: u32, block_offset: u32, write_size: u32) -> Result<u32, i32> {  
    let mut buf = vec![0u8; STORAGE_BLOCK_SIZE];  
  
    // Check if the write operation would overflow the block size  
    if block_offset + write_size > STORAGE_BLOCK_SIZE as u32 {  
        return Ok(0);  
    }  
  
    // Perform a partial block write  
    if !(block_offset == 0 && write_size == STORAGE_BLOCK_SIZE as u32) {  
        let read_ret = read_blocks(&mut buf, block_num, 1)?;  
        if read_ret != STORAGE_BLOCK_SIZE as u32 {  
            return Ok(0);  
        }  
    }  
  
    // Copy data to the buffer at the specified offset  
    buf[block_offset as usize..(block_offset + write_size) as usize].copy_from_slice(&data[..write_size as usize]);  
  
    // Write the buffer back to the block  
    let ret = write_blocks(&buf, block_num, 1)?;  
    if ret >= write_size {  
        Ok(write_size)  
    } else {  
        Ok(ret)  
    }  
} 
  
fn flush_dir_data_to_storage() {  
    DIR_DATA.with(|dir_data| {  
        let dir_data = dir_data.borrow();  
        let result = write_blocks(&dir_data[..], 0, DIR_DATA_NUM_BLOCKS as u32);  
        if let Err(e) = result {  
            eprintln!("Failed to write directory data to storage: {}", e);  
        }  
    });  
}
  
fn read_dir_data_from_storage() {  
    DIR_DATA.with(|dir_data| {  
        let mut dir_data = dir_data.borrow_mut();  
        let result = read_blocks(&mut dir_data[..], 0, DIR_DATA_NUM_BLOCKS as u32);  
        if let Err(e) = result {  
            eprintln!("Failed to read directory data from storage: {}", e);  
        }  
    });  
}  
  
fn update_file_in_directory(file: &File) -> Result<(), i32> {  
    let dir_data_off = file.dir_data_off;  
    let filename_size = file.filename.len();  
      
    if filename_size > MAX_FILENAME_SIZE {  
        return Err(ERR_INVALID);  
    }  
      
    if (dir_data_off + filename_size + 15) > DIR_DATA_SIZE {  
        return Err(ERR_MEMORY);  
    }  
  
    DIR_DATA.with(|dir_data| {  
        let mut dir_data = dir_data.borrow_mut();  
        let mut offset = dir_data_off;  
  
        // Write the filename size (u16)  
        dir_data[offset..offset + 2].copy_from_slice(&(filename_size as u16).to_le_bytes());  
        offset += 2;  
  
        // Write the filename  
        dir_data[offset..offset + filename_size].copy_from_slice(file.filename.as_bytes());  
        offset += filename_size;  
  
        // Null terminator for the filename  
        dir_data[offset] = 0;  
        offset += 1;  
  
        // Write the start_block (u32)  
        dir_data[offset..offset + 4].copy_from_slice(&file.start_block.to_le_bytes());  
        offset += 4;  
  
        // Write the num_blocks (u32)  
        dir_data[offset..offset + 4].copy_from_slice(&file.num_blocks.to_le_bytes());  
        offset += 4;  
  
        // Write the size (u32)  
        dir_data[offset..offset + 4].copy_from_slice(&file.size.to_le_bytes());  
    });  
  
    Ok(())  
}  

fn add_file_to_directory(file: &mut File) -> Result<(), i32> {  
    DIR_DATA_PTR.with(|dir_data_ptr| {  
        let mut dir_data_ptr = dir_data_ptr.borrow_mut();  
        file.dir_data_off = *dir_data_ptr;  
    });  
  
    let ret = update_file_in_directory(file);  
    if let Err(e) = ret {  
        eprintln!("Error: add_file_to_directory: couldn't update file info in directory");  
        return Err(e);  
    }  
  
    DIR_DATA_PTR.with(|dir_data_ptr| {  
        let mut dir_data_ptr = dir_data_ptr.borrow_mut();  
        *dir_data_ptr += file.filename.len() + 15;  
    });  
  
    DIR_DATA.with(|dir_data| {  
        let mut dir_data = dir_data.borrow_mut();  
        let num_files_offset = 4;  
        let num_files = u16::from_le_bytes([dir_data[num_files_offset], dir_data[num_files_offset + 1]]);  
        let new_num_files = num_files + 1;  
        dir_data[num_files_offset..num_files_offset + 2].copy_from_slice(&new_num_files.to_le_bytes());  
    });  
  
    flush_dir_data_to_storage();  
  
    Ok(())  
}  

  
fn expand_existing_file(fd: usize, needed_blocks: u32) -> Result<(), i32> {  

    let file_option = FILE_ARRAY.with(|file_array| file_array.borrow()[fd as usize].clone());  
    let file_rc = match file_option {  
        Some(file_rc) => file_rc,  
        None => {  
            eprintln!("Error: file_system_write_to_file: invalid fd");  
            return Err(ERR_INVALID); // Return 0 to mirror the original C code behavior  
        }  
    };  
  
    let mut file = RefCell::borrow_mut(&file_rc); 

    let mut found = true;  
  
    FILE_LIST.with(|file_list| {  
        let file_list = file_list.borrow();  
        for node in file_list.iter() {  
            let node_file = node.borrow();  
            if node_file.start_block >= (file.start_block + file.num_blocks) &&  
               node_file.start_block < (file.start_block + file.num_blocks + needed_blocks) {  
                found = false;  
                break;  
            }  
        }  
    });  
  
    if found {  
        let partition_num_blocks = PARTITION_NUM_BLOCKS.with(|n| *n.borrow());  
  
        if file.start_block + file.num_blocks + needed_blocks >= partition_num_blocks {  
            return Err(ERR_FOUND);  
        }  
  
        let zero_buf = [0u8; STORAGE_BLOCK_SIZE];  
        for i in 0..needed_blocks {  
            write_blocks(&zero_buf, file.start_block + file.num_blocks + i, 1)?;  
        }  
  
        file.num_blocks += needed_blocks;  
        return Ok(());  
    } else {  
        return Err(ERR_FOUND);  
    }  
}  

  
fn expand_empty_file(fd: usize, needed_blocks: u32) -> Result<(), i32> {  
    // Figure out if we have enough empty blocks to allocate.  
    // We will allocate space only after the last file.  
    let mut start_block = DIR_DATA_NUM_BLOCKS as u32;  
  
    FILE_LIST.with(|file_list| {  
        let file_list = file_list.borrow();  
        for node in file_list.iter() {  
            let node_file = node.borrow();  
            if node_file.start_block >= start_block {  
                start_block = node_file.start_block + node_file.num_blocks;  
            }  
        }  
    });  
  
    let partition_num_blocks = PARTITION_NUM_BLOCKS.with(|n| *n.borrow());  
  
    if start_block + needed_blocks >= partition_num_blocks {  
        return Err(ERR_FOUND);  
    }  
  
    // Zero out the new blocks  
    let zero_buf = [0u8; STORAGE_BLOCK_SIZE];  
    for i in 0..needed_blocks {  
        write_blocks(&zero_buf, start_block + i, 1)?;  
    }  
  
    let file_option = FILE_ARRAY.with(|file_array| file_array.borrow()[fd as usize].clone());  
    let file_rc = match file_option {  
        Some(file_rc) => file_rc,  
        None => {  
            eprintln!("Error: file_system_write_to_file: invalid fd");  
            return Err(ERR_INVALID); // Return 0 to mirror the original C code behavior  
        }  
    };  
  
    let mut file = RefCell::borrow_mut(&file_rc);  

    file.start_block = start_block;  
    file.num_blocks = needed_blocks;  
  
    Ok(())  
}  
  
fn expand_file_size(fd: usize, size: u32) -> Result<(), i32> {
    let file_cell = FILE_ARRAY.with(|file_array|
        file_array.borrow_mut()[fd].clone()
    ).unwrap();
    let mut file = RefCell::borrow_mut(&file_cell);
    if file.size >= size {  
        return Ok(());  
    }  
  
    let (empty_file, needed_size) = if file.size == 0 {  
        (true, size)  
    } else {  
        (false, size - file.size)  
    };  
  
    // First check if there's enough space in the last block  
    let leftover = STORAGE_BLOCK_SIZE as u32 - (file.size % STORAGE_BLOCK_SIZE as u32);  
    if (leftover != STORAGE_BLOCK_SIZE as u32) && (leftover >= needed_size) {  
        update_file_size(&mut file, size)?;  
        let ret = update_file_in_directory(&file);  
        if let Err(e) = ret {  
            eprintln!("Error: expand_file_size: couldn't update file info in directory: {:?}", e);  
        }  
        flush_dir_data_to_storage();  
        return Ok(());  
    }  
  
    let mut needed_blocks = needed_size / STORAGE_BLOCK_SIZE as u32;  
    if needed_size % STORAGE_BLOCK_SIZE as u32 != 0 {  
        needed_blocks += 1;  
    }  
  
    drop(file);
    let ret = if empty_file {  
        expand_empty_file(fd, needed_blocks)  
    } else {  
        expand_existing_file(fd, needed_blocks)  
    };  

    let mut file = RefCell::borrow_mut(&file_cell);
  
    if ret.is_ok() {  
        update_file_size(&mut file, size)?;  
        let ret = update_file_in_directory(&file);  
        if let Err(e) = ret {  
            eprintln!("Error: expand_file_size: couldn't update file info in directory: {:?}", e);  
        }  
        flush_dir_data_to_storage();  
    }  
  
    ret  
}  
  
fn update_file_size(file: &mut File, size: u32) -> Result<(), i32> {  
    file.size = size;  
    Ok(())  
}  
  
fn release_file_blocks(_file: &File) {  
    // No-op  
}  

