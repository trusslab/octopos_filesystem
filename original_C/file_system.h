#define MAX_NUM_FD 64
#define FILE_OPEN_MODE 0
#define FILE_OPEN_CREATE_MODE 1

#define STORAGE_BLOCK_SIZE 512
#define DIR_DATA_NUM_BLOCKS 2
#define DIR_DATA_SIZE (STORAGE_BLOCK_SIZE * DIR_DATA_NUM_BLOCKS)

#define MAX_FILENAME_SIZE 256

#define ERR_INVALID -2
#define ERR_EXIST -5
#define ERR_MEMORY -6
#define ERR_FOUND -7

#include <stdio.h>
#include <string.h>
#include <stdint.h>

uint32_t file_system_open_file(char *filename, uint32_t mode);
uint32_t file_system_write_to_file(uint32_t fd, uint8_t *data, uint32_t size, uint32_t offset);
uint32_t file_system_read_from_file(uint32_t fd, uint8_t *data, uint32_t size, uint32_t offset);
int file_system_close_file(uint32_t fd);
void initialize_file_system(uint32_t _partition_num_blocks);
void close_file_system(void);

