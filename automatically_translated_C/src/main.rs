#![allow(unused_variables)]

mod file_system;
use file_system::{*};

const STORAGE_BOOT_PARTITION_SIZE: u32 = 200000;

fn write_file(file_name: &str, data: &[u8], data_len: u32) {  
    let fd = match file_system_open_file(file_name, FILE_OPEN_CREATE_MODE) {  
        Ok(fd) if fd != 0 => fd,  
        _ => {  
            println!("Failed to open/create file");  
            return;  
        }  
    };  
  
    if file_system_write_to_file(fd, data, data_len, 0).unwrap_or(0) != data_len {  
        println!("Failed to write everything to file");  
    }  
  
    if let Err(_) = file_system_close_file(fd) {  
        println!("Failed to close file");  
    }  
}  

fn assert_file_eq(file_name: &str, data: &[u8], data_len: u32, cmp_buffer: &mut [u8]) {  
    let fd = match file_system_open_file(file_name, FILE_OPEN_MODE) {  
        Ok(fd) if fd != 0 => fd,  
        _ => {  
            println!("Failed to open file");  
            return;  
        }  
    };  
  
    if file_system_read_from_file(fd, cmp_buffer, data_len, 0).unwrap_or(0) != data_len {  
        println!("Failed to read everything from file");  
    }  
  
    if let Err(_) = file_system_close_file(fd) {  
        println!("Failed to close file");  
    }  
  
    if &data[..data_len as usize] != &cmp_buffer[..data_len as usize] {  
        println!("File data was incorrect");  
    }  
}  

fn test_fs() {  
    initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);  
  
    let text = "This is text in hello";  
    write_file("hello", text.as_bytes(), text.len() as u32);  
  
    let random_text = "aljksdjfalskdfja;slkdfja;s";  
    write_file("random", random_text.as_bytes(), random_text.len() as u32);  
  
    let testing_text = "TESTING TESTING";  
    write_file("testing", testing_text.as_bytes(), testing_text.len() as u32);  
  
    let not_testing_text = "No testing";  
    write_file("not_testing", not_testing_text.as_bytes(), not_testing_text.len() as u32);  
  
    let mut file_cmp_buff = vec![0u8; 500];  
  
    assert_file_eq("hello", text.as_bytes(), text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("random", random_text.as_bytes(), random_text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("testing", testing_text.as_bytes(), testing_text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("not_testing", not_testing_text.as_bytes(), not_testing_text.len() as u32, &mut file_cmp_buff);  
  
    close_file_system();  
    initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);  
  
    assert_file_eq("hello", text.as_bytes(), text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("random", random_text.as_bytes(), random_text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("testing", testing_text.as_bytes(), testing_text.len() as u32, &mut file_cmp_buff);  
    assert_file_eq("not_testing", not_testing_text.as_bytes(), not_testing_text.len() as u32, &mut file_cmp_buff);  
}  


fn main() {
    test_fs();
}
