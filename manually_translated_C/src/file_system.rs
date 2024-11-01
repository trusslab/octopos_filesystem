use std::{collections::HashMap, ffi::{CStr, CString}, fs, io::{Read, Write}, path::Path, process::exit};

const MAX_NUM_FD: usize = 64;
pub const FILE_OPEN_MODE: u32 = 0;
pub const FILE_OPEN_CREATE_MODE: u32 = 1;

const STORAGE_BLOCK_SIZE: usize = 512;
const DIR_DATA_NUM_BLOCKS: usize = 2;
const DIR_DATA_SIZE: usize = STORAGE_BLOCK_SIZE * DIR_DATA_NUM_BLOCKS;

const MAX_FILENAME_SIZE: usize = 256;

pub const ERR_INVALID: i32 = -2;
pub const ERR_EXIST: i32 = -5;
pub const ERR_MEMORY: i32 = -6;
pub const ERR_FOUND: i32 = -7;

enum FileRef<'a> {
    Ino(u32),
    Ref(&'a mut File),
}

struct File {
    filename: CString,
    start_block: u32,
    num_blocks: u32,
    size: u32,
    dir_data_off: u32,
    opened: bool,
}

pub struct FileSystem {
    file_array: [u32; MAX_NUM_FD],
    fd_bitmap: [u8; MAX_NUM_FD / 8],
    next_ino: u32,
    files: HashMap<u32, File>,
    dir_data: [u8; DIR_DATA_SIZE],
    dir_data_ptr: usize,
    partition_num_blocks: u32,
}

impl FileSystem {
    pub fn initialize_file_system(partition_num_blocks: u32) -> FileSystem {
        let mut fs = FileSystem {
            file_array: [0; MAX_NUM_FD],
            fd_bitmap: [0; MAX_NUM_FD / 8],
            next_ino: 1,
            files: HashMap::new(),
            dir_data: [0; DIR_DATA_SIZE],
            dir_data_ptr: 0,
            partition_num_blocks,
        };

        if MAX_NUM_FD % 8 != 0 {
            println!("Error: initialize_file_system: MAX_NUM_FD must be divisible by 8");
            exit(-1);
        }

        fs.fd_bitmap[0] = 0x00000001;

        read_dir_data_from_storage(&mut fs.dir_data);

        if fs.dir_data[0..4] == [b'$', b'%', b'^', b'&'] {
            let num_files = u16::from_ne_bytes(fs.dir_data[4..6].try_into().unwrap());

            fs.dir_data_ptr = 6;
            for i in 0..num_files {
                let dir_data_off = fs.dir_data_ptr;
                if fs.dir_data_ptr + 2 > DIR_DATA_SIZE {
                    break;
                }

                let filename_size = u16::from_ne_bytes(fs.dir_data[fs.dir_data_ptr..(fs.dir_data_ptr + 2)].try_into().unwrap());
                if fs.dir_data_ptr + filename_size as usize + 15 > DIR_DATA_SIZE {
                    break;
                }
                fs.dir_data_ptr += 2;

                if filename_size > MAX_FILENAME_SIZE as u16 {
                    break;
                }

                let mut found_null = false;
                let filename_vec = Vec::from_iter(
                    fs.dir_data[fs.dir_data_ptr..(fs.dir_data_ptr + MAX_FILENAME_SIZE)].iter().take_while(|b| { **b != b'\0' }).copied()
                );
                let filename = CString::new(filename_vec).unwrap();
                fs.dir_data_ptr += filename_size as usize + 1;

                let start_block = u32::from_ne_bytes(fs.dir_data[fs.dir_data_ptr..(fs.dir_data_ptr + 4)].try_into().unwrap());
                fs.dir_data_ptr += 4;
                let num_blocks = u32::from_ne_bytes(fs.dir_data[fs.dir_data_ptr..(fs.dir_data_ptr + 4)].try_into().unwrap());
                fs.dir_data_ptr += 4;
                let size = u32::from_ne_bytes(fs.dir_data[fs.dir_data_ptr..(fs.dir_data_ptr + 4)].try_into().unwrap());
                fs.dir_data_ptr += 4;

                let file = File {
                    filename,
                    start_block,
                    num_blocks,
                    size,
                    dir_data_off: dir_data_off as u32,
                    opened: false,
                };

                let _ = fs.add_file_to_list(file);
            }
        } else {
            fs.dir_data[0..6].copy_from_slice(&[b'$', b'%', b'^', b'&', 0, 0]);
            fs.dir_data_ptr = 6;
            fs.flush_dir_data_to_storage();
        }

        fs
    }

    pub fn close_file_system(&self) {
        self.flush_dir_data_to_storage();
    }

    fn get_next_ino(&mut self) -> u32 {
        self.next_ino += 1;
        self.next_ino - 1
    }

    fn update_file_in_directory(&mut self, file_ref: FileRef) -> Result<(), i32> {
        // I use file ref here because in some cases the file is not in files yet and thus has no ino and in some it is and passing a &mut causes issues.
        let file = match file_ref {
            FileRef::Ino(ino) => self.files.get_mut(&ino).unwrap(),
            FileRef::Ref(fref) => fref,
        };
        let mut dir_data_off = file.dir_data_off as usize;
        let filename_size = file.filename.count_bytes();

        if filename_size > MAX_FILENAME_SIZE {
            return Err(ERR_INVALID);
        }

        if (dir_data_off + filename_size + 15) > DIR_DATA_SIZE { 
            return Err(ERR_MEMORY);
        }

        self.dir_data[dir_data_off..(dir_data_off + 2)].copy_from_slice(&(filename_size as u16).to_ne_bytes());
        dir_data_off += 2;

        self.dir_data[dir_data_off..(dir_data_off + filename_size + 1)].copy_from_slice(file.filename.as_bytes_with_nul());
        dir_data_off += filename_size + 1;

        self.dir_data[dir_data_off..(dir_data_off + 4)].copy_from_slice(&file.start_block.to_ne_bytes());
        dir_data_off += 4;

        self.dir_data[dir_data_off..(dir_data_off + 4)].copy_from_slice(&file.num_blocks.to_ne_bytes());
        dir_data_off += 4;

        self.dir_data[dir_data_off..(dir_data_off + 4)].copy_from_slice(&file.size.to_ne_bytes());

        Ok(())
    }

    fn add_file_to_directory(&mut self, file: &mut File) -> Result<(), i32> {
        file.dir_data_off = self.dir_data_ptr as u32;

        if let Err(e) = self.update_file_in_directory(FileRef::Ref(file)) {
            // __func__ does not exist in rust without custom macros so I just put the function name
            println!("Error: add_file_to_directory: couldn't update file info in directory");
            return  Err(e);
        }

        self.dir_data_ptr += file.filename.count_bytes() + 15;

        // increment number of files
        self.dir_data[4] += 1;

        self.flush_dir_data_to_storage();

        Ok(())
    }

    fn get_unused_fd(&mut self) -> Result<u32, i32> {
        for i in 0..(MAX_NUM_FD / 8) {
            if self.fd_bitmap[i] == 0xFF {
                continue;
            }

            let mut mask: u8 = 0b00000001;
            for j in 0..8 {
                if (self.fd_bitmap[i] | !mask) != 0xFF {
                    self.fd_bitmap[i] |= mask;
                    return Ok(((i * 8) + j + 1) as u32);
                }

                mask = mask << 1;
            }
        }

        Err(ERR_EXIST)
    }

    fn mark_fd_unused(&mut self, fd: u32) {
        let fd = fd - 1;
        if fd >= MAX_NUM_FD as u32 {
            println!("Error: mark_fd_unused: invalid fd {fd}");
            return
        }

        let byte_off = fd / 8;
        let bit_off = fd % 8;

        let mut mask: u8 = 0b00000001;
        mask = mask << bit_off;

        self.fd_bitmap[byte_off as usize] &= !mask;
    }

    pub fn file_system_open_file(&mut self, filename: &CStr, mode: u32) -> Result<u32, ()> {
        if !(mode == FILE_OPEN_MODE || mode == FILE_OPEN_CREATE_MODE) {
            println!("Error: invalid mode for opening a file");
            return Err(());
        }

        let mut ino = 0;
        for (file_ino, file) in &self.files {
            if file.filename.as_c_str() == filename {
                if file.opened {
                    return Err(());
                }
                ino = *file_ino;
                break;
            }
        }

        if ino == 0 && mode == FILE_OPEN_CREATE_MODE {
            let mut file = File { 
                filename: filename.into(), 
                start_block: 0, 
                num_blocks: 0, 
                size: 0, 
                dir_data_off: 0, 
                opened: false,
            };

            if self.add_file_to_directory(&mut file).is_err() {
                return Err(());
            }            

            if let Ok(new_ino) = self.add_file_to_list(file) {
                ino = new_ino;
            }
        }

        if ino != 0 {
            let Ok(fd) = self.get_unused_fd() else {
                return Err(());
            };
            let fd = fd as usize;

            if fd == 0 || fd >= MAX_NUM_FD {
                return Err(());
            }

            self.file_array[fd] = ino;
            
            self.files.get_mut(&ino).unwrap().opened = true;
            
            return Ok(fd as u32);
        }

        Err(())
    }

    fn add_file_to_list(&mut self, file: File) -> Result<u32, i32> {
        let ino = self.get_next_ino();
        self.files.insert(ino, file);
        Ok(ino)
    }

    pub fn file_system_close_file(&mut self, fd_32: u32) -> Result<(), i32> {
        let fd = fd_32 as usize;
        if fd == 0 || fd >= MAX_NUM_FD {
            println!("Error: file_system_close_file: fd is 0 or too large ({fd})");
            return Err(ERR_INVALID);
        }

        if self.file_array[fd] == 0 {
            println!("Error: file_system_close_file: invalid fd");
            return Err(ERR_INVALID);
        }

        // I noticed that the original code may have out of bounds read here if fd is MAX_NUM_FD so this code will probably panic in that case.
        // To fix it you probably need to treat fd as fd - 1 for indexing into file_array
        let file = self.files.get_mut(&self.file_array[fd]).unwrap();

        if !file.opened {
            println!("Error: file_system_close_file: file not opened!");
            return Err(ERR_INVALID);
        }

        file.opened = false;
        self.file_array[fd] = 0;
        self.mark_fd_unused(fd_32);

        Ok(())
    }

    pub fn file_system_read_from_file(&self, fd: u32, data: &mut [u8], offset: u32) -> Result<u32, ()> {
        let fd = fd as usize;
        if fd == 0 || fd >= MAX_NUM_FD {
            println!("Error: file_system_read_from_file: fd is 0 or too large ({fd})");
        }

        if self.file_array[fd] == 0 {
            println!("Error: file_system_read_from_file: invalid fd");
            return Err(());
        }

        // I noticed that the original code may have out of bounds read here if fd is MAX_NUM_FD so this code will probably panic in that case.
        // To fix it you probably need to treat fd as fd - 1 for indexing into file_array
        let file = self.files.get(&self.file_array[fd]).unwrap();

        if !file.opened {
            println!("Error: file_system_read_from_file: file not opened!");
            return Err(());
        }

        if offset >= file.size {
            return Err(());
        }

        let mut size = data.len() as u32;
        if file.size < (offset + size) {
            size = file.size - offset;
        }

        let mut block_num = offset / STORAGE_BLOCK_SIZE as u32;
        let mut block_offset = offset % STORAGE_BLOCK_SIZE as u32;
        let mut read_size = 0;
        let mut next_read_size = STORAGE_BLOCK_SIZE as u32 - block_offset;
        if next_read_size > size {
            next_read_size = size;
        }

        while read_size < size {
            let ret = read_from_block(&mut data[(read_size as usize)..((read_size + next_read_size) as usize)], block_num, block_offset);
            if ret != next_read_size {
                read_size += ret;
                break;
            }

            read_size += next_read_size;
            block_num += 1;
            block_offset = 0;
            if (size - read_size) as usize >= STORAGE_BLOCK_SIZE {
                next_read_size = STORAGE_BLOCK_SIZE as u32 - block_offset;
            } else {
                next_read_size = size - read_size;
            }
        }

        Ok(read_size)
    }

    fn expand_existing_file(&mut self, ino: u32, needed_blocks: u32) -> Result<(), i32> {
        let mut found = true;

        for file in self.files.values() {
            if file.start_block >= file.start_block + file.num_blocks && file.start_block < file.start_block + file.num_blocks + needed_blocks {
                found = false;
                break;
            }
        }

        let file = self.files.get_mut(&ino).unwrap();
        if found {
            if file.start_block + file.num_blocks + needed_blocks >= self.partition_num_blocks {
                return Err(ERR_FOUND);
            }

            let zero_buf = [0; STORAGE_BLOCK_SIZE];
            for i in 0..needed_blocks {
                write_blocks(&zero_buf, file.start_block + file.num_blocks + i, 1);
            }

            file.num_blocks = needed_blocks;

            return Ok(());
        } else {
            return Err(ERR_FOUND);
        }
    }

    fn expand_empty_file(&mut self, ino: u32, needed_blocks: u32) -> Result<(), i32> {
        let mut start_block = DIR_DATA_NUM_BLOCKS as u32;

        for file in self.files.values() {
            if file.start_block >= start_block {
                start_block = file.start_block + file.num_blocks;
            }
        }

        if start_block + needed_blocks >= self.partition_num_blocks {
            return Err(ERR_FOUND);
        }

        let zero_buf = [0; STORAGE_BLOCK_SIZE];
        for i in 0..needed_blocks {
            write_blocks(&zero_buf, start_block + i, 1);
        }

        let file = self.files.get_mut(&ino).unwrap();
        file.start_block = start_block;
        file.num_blocks = needed_blocks;

        Ok(())
    }

    fn expand_file_size(&mut self, ino: u32, size: u32) -> Result<(), i32> {
        let file = self.files.get_mut(&ino).unwrap();

        if file.size >= size {
            return Ok(());
        }
        
        let empty_file;
        let needed_size;
        if file.size == 0 {
            empty_file = true;
            needed_size = size;
        } else {
            empty_file = false;
            needed_size = size - file.size;
        }

        let leftover = STORAGE_BLOCK_SIZE - (file.size as usize % STORAGE_BLOCK_SIZE);

        if !(leftover != STORAGE_BLOCK_SIZE && leftover >= needed_size as usize) {
            let mut needed_blocks = needed_size as usize / STORAGE_BLOCK_SIZE;
            if needed_size as usize % STORAGE_BLOCK_SIZE != 0 {
                needed_blocks += 1;
            }

            if empty_file {
                self.expand_empty_file(ino, needed_blocks as u32)?;
            } else {
                self.expand_existing_file(ino, needed_blocks as u32)?;
            }
        }

        // Have to reget file to avoid 2 mutable borrows.
        let file = self.files.get_mut(&ino).unwrap();

        file.size = size;
        if let Err(e) = self.update_file_in_directory(FileRef::Ino(ino)) {
            println!("Error: expand_file_size: couldn't update file info in directory.");
        }

        self.flush_dir_data_to_storage();
        return Ok(());
    }

    pub fn file_system_write_to_file(&mut self, fd: u32, data: &[u8], offset: u32) -> Result<u32, ()> {
        let fd = fd as usize;
        if fd == 0 || fd >= MAX_NUM_FD {
            println!("Error: file_system_write_to_file: fd is 0 or too large ({fd})");
        }

        if self.file_array[fd] == 0 {
            println!("Error: file_system_write_to_file: invalid fd");
            return Err(());
        }

        // I noticed that the original code may have out of bounds read here if fd is MAX_NUM_FD so this code will probably panic in that case.
        // To fix it you probably need to treat fd as fd - 1 for indexing into file_array
        let file = self.files.get(&self.file_array[fd]).unwrap();

        if !file.opened {
            println!("Error: file_system_write_to_file: file not opened!");
            return Err(());
        }

        let mut size = data.len() as u32;

        if file.size < (offset + size) {
            if offset > file.size {
                println!("Error: file_system_write_to_file: invalid offset (offset = {offset}, file->size = {}", file.size);
                return Err(());
            }

            let _ = self.expand_file_size(self.file_array[fd], offset + size);
        }

        // Have to reget to avoid multiple borrows
        let file = self.files.get(&self.file_array[fd]).unwrap();
        if offset >= file.size {
            return Err(());
        }

        if file.size < (offset + size) {
            size = file.size - offset;
        }

        let mut block_num = offset / STORAGE_BLOCK_SIZE as u32;
        let mut block_offset = offset % STORAGE_BLOCK_SIZE as u32;
        let mut written_size = 0;
        let mut next_write_size = STORAGE_BLOCK_SIZE as u32 - block_offset;
        if next_write_size > size {
            next_write_size = size;
        }
        let mut ret = 0;

        while written_size < size {
            ret = write_to_block(&data[(written_size as usize)..((written_size + next_write_size) as usize)], file.start_block + block_num, block_offset);

            if ret != next_write_size {
                written_size += ret;
                break;
            }
            written_size += next_write_size;
            block_num += 1;
            block_offset = 0;
            if size - written_size >= STORAGE_BLOCK_SIZE as u32 {
                next_write_size = STORAGE_BLOCK_SIZE as u32 - block_offset;
            } else {
                next_write_size = size - written_size;
            }
        }

        Ok(written_size)
    }

    fn flush_dir_data_to_storage(&self) {
        write_blocks(&self.dir_data, 0, DIR_DATA_NUM_BLOCKS as u32);
    }
}

fn read_dir_data_from_storage(dir_data: &mut [u8]) {
    read_blocks(dir_data, 0, DIR_DATA_NUM_BLOCKS as u32);
}

fn read_from_block(data: &mut [u8], block_num: u32, block_offset: u32) -> u32 {
    if block_offset as usize + data.len() > STORAGE_BLOCK_SIZE {
        return 0;
    }

    let mut buf = [0; STORAGE_BLOCK_SIZE];

    let ret = read_blocks(&mut buf, block_num, 1);
    if ret as usize != STORAGE_BLOCK_SIZE {
        return 0;
    }

    data.copy_from_slice(&buf[(block_offset as usize)..(block_offset as usize + data.len())]);

    return data.len() as u32;
}

fn read_blocks(data: &mut [u8], start_block: u32, num_blocks: u32) -> u32 {
    let mut read = 0;
    for i in 0..num_blocks {
        let block_num = start_block + i;
        let block_name = format!("block{block_num}.txt");
        if !Path::new(&block_name).exists() {
            write_blocks(&[0; STORAGE_BLOCK_SIZE], start_block + i, 1);
        }

        let Ok(mut file) = fs::File::open(&block_name) else {
            println!("Error: Failed to open block file {block_name}");
            return read;
        };

        if file.read_exact(&mut data[(i as usize * STORAGE_BLOCK_SIZE)..((i as usize + 1) * STORAGE_BLOCK_SIZE)]).is_err() {
            return read;
        }

        read += STORAGE_BLOCK_SIZE as u32;
    }
    return read;
}

fn write_blocks(data: &[u8], start_block: u32, num_blocks: u32) -> u32 {
    let mut written = 0;
    for i in 0..num_blocks {
        let block_num = start_block + i;
        let block_name = format!("block{block_num}.txt");
        let Ok(mut file) = fs::File::create(&block_name) else {
            println!("Error: Failed to open block file {block_name}");
            return written;
        };

        if file.write_all(&data[(i as usize * STORAGE_BLOCK_SIZE)..((i as usize + 1) * STORAGE_BLOCK_SIZE)]).is_err() {
            return written;
        }

        written += STORAGE_BLOCK_SIZE as u32;
    }
    return written;
}

fn write_to_block(data: &[u8], block_num: u32, block_offset: u32) -> u32 {
    if block_offset as usize + data.len() > STORAGE_BLOCK_SIZE {
        return 0;
    }

    let mut buf = [0; STORAGE_BLOCK_SIZE];

    // Partial block write
    if !(block_offset == 0 && data.len() == STORAGE_BLOCK_SIZE) {
        let read_ret = read_blocks(&mut buf, block_num, 1);
        if read_ret != STORAGE_BLOCK_SIZE as u32 {
            return 0;
        }
    }

    buf[(block_offset as usize)..(block_offset as usize + data.len())].copy_from_slice(data);

    let ret = write_blocks(&buf, block_num, 1);

    if ret >= data.len() as u32 {
        return data.len() as u32;
    } else {
        return ret;
    }
}