use std::ffi::CStr;

use file_system::{FileSystem, FILE_OPEN_CREATE_MODE, FILE_OPEN_MODE};

mod file_system;

const STORAGE_BOOT_PARTITION_SIZE: u32 = 200000;

fn write_file(fs: &mut FileSystem, file_name: &CStr, data: &[u8]) {
	let fd = fs.file_system_open_file(file_name, FILE_OPEN_CREATE_MODE);
	let Ok(fd) = fd else {
		println!("Failed to open/create file");
		return;
	};

	if !fs.file_system_write_to_file(fd, data, 0).is_ok_and(|wrote| wrote as usize == data.len()) {
		println!("Failed to write everything to file");
	}

	if fs.file_system_close_file(fd).is_err() {
		println!("Failed to close file");
	}
}

// cmp_buffer must be at least data_len in size
fn assert_file_eq(fs: &mut FileSystem, file_name: &CStr, data: &[u8], cmp_buffer: &mut [u8]) {
	let Ok(fd ) = fs.file_system_open_file(file_name, FILE_OPEN_MODE) else {
		println!("Failed to open file\n");
		return;
	};

	if !fs.file_system_read_from_file(fd, &mut cmp_buffer[0..data.len()], 0).is_ok_and(|read| read as usize == data.len()) {
		println!("Failed to read everything from file\n");
	}

	if fs.file_system_close_file(fd).is_err() {
		println!("Failed to close file\n");
	}

	if data == &cmp_buffer[0..data.len()] {
		println!("File data was incorrect\n");
	}
}

fn test_fs() {
	let mut fs = FileSystem::initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);

	let text = "This is text in hello";
	write_file(&mut fs, c"hello", text.as_bytes());

	let random_text = "aljksdjfalskdfja;slkdfja;s";
	write_file(&mut fs, c"random", random_text.as_bytes());

	let testing_text = "TESTING TESTING";
	write_file(&mut fs, c"testing", testing_text.as_bytes());

	let not_testing_text = "No testing";
	write_file(&mut fs, c"not_testing", not_testing_text.as_bytes());

	let mut file_cmp_buff = [0; 500];
	assert_file_eq(&mut fs, c"hello", text.as_bytes(), &mut file_cmp_buff);

	assert_file_eq(&mut fs, c"random", random_text.as_bytes(), &mut file_cmp_buff);

	assert_file_eq(&mut fs, c"testing", testing_text.as_bytes(), &mut file_cmp_buff);

	assert_file_eq(&mut fs, c"not_testing", not_testing_text.as_bytes(), &mut file_cmp_buff);

	fs.close_file_system();

    drop(fs);

	let mut fs = FileSystem::initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);

	assert_file_eq(&mut fs, c"hello", text.as_bytes(), &mut file_cmp_buff);

	assert_file_eq(&mut fs, c"random", random_text.as_bytes(), &mut file_cmp_buff);

    assert_file_eq(&mut fs, c"testing", testing_text.as_bytes(), &mut file_cmp_buff);

	assert_file_eq(&mut fs, c"not_testing", not_testing_text.as_bytes(), &mut file_cmp_buff);
}


fn main() {
	test_fs();
}	