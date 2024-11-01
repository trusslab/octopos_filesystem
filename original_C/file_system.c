/********************************************************************
 * Copyright (c) 2019 - 2023, The OctopOS Authors
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 ********************************************************************/
/* OctopOS file system
 *
 * This file is used the OS, the installer, and the bootloader for storage.
 * We use macros ROLE_... to specialize, i.e., to compile only the needed code
 * for each.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <unistd.h>
#include <stdint.h>
#include <fcntl.h>
#include <stdbool.h>
#include "file_system.h"

#define MAX_FILENAME_SIZE	256

/* FIXME: do we need access control for this FS? Currently, anyone can
 * read/write to any file.
 *
 * Answer1: For the OS using this FS for the boot partition, It should be
 * enough to make the files read-only.
 */

/* FIXME: hard-coded */
uint32_t partition_num_blocks;

struct file {
	char filename[MAX_FILENAME_SIZE];
	uint32_t start_block; /* First block of file in the partition */
	uint32_t num_blocks;
	uint32_t size; /* in bytes */
	uint32_t dir_data_off;
	bool opened;
};

/* FIXME: use per-process fd */
// #define MAX_NUM_FD	64 /* must be divisible by 8 */
uint8_t fd_bitmap[MAX_NUM_FD / 8];

struct file *file_array[MAX_NUM_FD];

struct file_list_node {
	struct file *file;
	struct file_list_node *next;
};

struct file_list_node *file_list_head;
struct file_list_node *file_list_tail;

uint8_t dir_data[DIR_DATA_SIZE];
int dir_data_ptr;

static int get_unused_fd(void)
{
	for (int i = 0; i < (MAX_NUM_FD / 8); i++) {
		if (fd_bitmap[i] == 0xFF)
			continue;

		uint8_t mask = 0b00000001;
		for (int j = 0; j < 8; j++) {
			if (((uint8_t) (fd_bitmap[i] | ~mask)) != 0xFF) {
				fd_bitmap[i] |= mask;
				return (i * 8) + j + 1;
			}

			mask = mask << 1;
		}
	}

	return ERR_EXIST;
}

static void mark_fd_as_unused(int _fd)
{
	int fd = _fd - 1;

	if (fd >= MAX_NUM_FD) {
		printf("Error: %s: invalid fd %d\n", __func__, fd);
		return;
	}

	int byte_off = fd / 8;
	int bit_off = fd % 8;

	uint8_t mask = 0b00000001;
	for (int i = 0; i < bit_off; i++)
		mask = mask << 1;

	fd_bitmap[byte_off] &= ~mask;
}

static int add_file_to_list(struct file *file)
{
	struct file_list_node *node =
		(struct file_list_node *) malloc(sizeof(struct file_list_node));
	if (!node)
		return ERR_MEMORY;

	node->file = file;
	node->next = NULL;

	if (file_list_head == NULL && file_list_tail == NULL) {
		/* first node */
		file_list_head = node;
		file_list_tail = node;
	} else {
		file_list_tail->next = node;
		file_list_tail = node;
	}

	return 0;
}

static uint32_t write_blocks(uint8_t *data, uint32_t start_block,
			     uint32_t num_blocks)
{

	char block_name[30];
	int written = 0;

	for (uint32_t i = 0; i < num_blocks; i++) {
		uint32_t block_num = start_block + i;
		sprintf(block_name, "block%d.txt", block_num);

		FILE *fptr = fopen(block_name, "w");
		if (fptr == NULL) {
			printf("Error: Failed to open block file\n");
			return written;
		}

		int ret = fwrite(data + i * STORAGE_BLOCK_SIZE, 1, STORAGE_BLOCK_SIZE, fptr);
		fclose(fptr);
		if (ret == 0) {
			fclose(fptr);
			return written;
		}
		written += ret;
	}

	return written;
}

static uint32_t read_blocks(uint8_t *data, uint32_t start_block,
			    uint32_t num_blocks)
{
	char block_name[30];
	int read = 0;

	for (uint32_t i = 0; i < num_blocks; i++) {
		uint32_t block_num = start_block + i;
		sprintf(block_name, "block%d.txt", block_num);

		FILE *fptr = fopen(block_name, "r");
		if (fptr == NULL) {
			char zero_buf[STORAGE_BLOCK_SIZE];
			memset(zero_buf, 0, STORAGE_BLOCK_SIZE);
			write_blocks(zero_buf, block_num, 1);
			fptr = fopen(block_name, "r");
		}
		if (fptr == NULL) {
			printf("Error: Failed to open block file %s\n", block_name);
			return read;
		}

		int ret = fread(data + i * STORAGE_BLOCK_SIZE, 1, STORAGE_BLOCK_SIZE, fptr);
		fclose(fptr);
		read += ret;
		if (ret != STORAGE_BLOCK_SIZE) {
			fclose(fptr);
			return read;
		}
	}

	return read;
}

static uint32_t read_from_block(uint8_t *data, uint32_t block_num,
				uint32_t block_offset, uint32_t read_size)
{
	uint8_t buf[STORAGE_BLOCK_SIZE];

	if (block_offset + read_size > STORAGE_BLOCK_SIZE)
		return 0;

	uint32_t ret = read_blocks(buf, block_num, 1);
	if (ret != STORAGE_BLOCK_SIZE)
		return 0;

	memcpy(data, buf + block_offset, read_size);

	return read_size;
}

static int write_to_block(uint8_t *data, uint32_t block_num,
			  uint32_t block_offset, uint32_t write_size)
{
	uint8_t buf[STORAGE_BLOCK_SIZE];

	if (block_offset + write_size > STORAGE_BLOCK_SIZE)
		return 0;

	/* partial block write */
	if (!(block_offset == 0 && write_size == STORAGE_BLOCK_SIZE)) {
		int read_ret = read_blocks(buf, block_num, 1);
		if (read_ret != STORAGE_BLOCK_SIZE)
			return 0;
	}

	memcpy(buf + block_offset, data, write_size);

	uint32_t ret = write_blocks(buf, block_num, 1);

	if (ret >= write_size)
		return write_size;
	else
		return ret;
}

static void flush_dir_data_to_storage(void)
{
	write_blocks(dir_data, 0, DIR_DATA_NUM_BLOCKS);
}

static void read_dir_data_from_storage(void)
{
	read_blocks(dir_data, 0, DIR_DATA_NUM_BLOCKS);
}

static int update_file_in_directory(struct file *file)
{
	int dir_data_off = file->dir_data_off;

	int filename_size = strlen(file->filename);
	if (filename_size > MAX_FILENAME_SIZE)
		return ERR_INVALID;

	if ((dir_data_off + filename_size + 15) > DIR_DATA_SIZE)
		return ERR_MEMORY;


	*((uint16_t *) &dir_data[dir_data_off]) = filename_size;
	dir_data_off += 2;

	strcpy((char *) &dir_data[dir_data_off], file->filename);
	dir_data_off += (filename_size + 1);

	*((uint32_t *) &dir_data[dir_data_off]) = file->start_block;
	dir_data_off += 4;

	*((uint32_t *) &dir_data[dir_data_off]) = file->num_blocks;	
	dir_data_off += 4;

	*((uint32_t *) &dir_data[dir_data_off]) = file->size;

	return 0;
}

static int add_file_to_directory(struct file *file)
{
	file->dir_data_off = dir_data_ptr;

	int ret = update_file_in_directory(file);
	if (ret) {
		printf("Error: %s: couldn't update file info in directory\n",
		       __func__);
		return ret;
	}

	dir_data_ptr += (strlen(file->filename) + 15);
	
	/* increment number of files */
	(*((uint16_t *) &dir_data[4]))++;

	flush_dir_data_to_storage();

	return 0;
}

static int expand_existing_file(struct file *file, uint32_t needed_blocks)
{
	/* Figure out if we have enough empty blocks to allocate.
	 * The empty blocks must be at the end of the file blocks.
	 */
	bool found = true;

	for (struct file_list_node *node = file_list_head; node;
	     node = node->next) {
		if ((node->file->start_block >= (file->start_block +
						 file->num_blocks)) &&
		    (node->file->start_block < (file->start_block +
						file->num_blocks +
						needed_blocks))) {
			found = false;
			break;
		}
	}

	if (found) {
		if (file->start_block + file->num_blocks + needed_blocks >=
		    partition_num_blocks)
			return ERR_FOUND;

		/* zero out the new blocks */
		uint8_t zero_buf[STORAGE_BLOCK_SIZE];
		memset(zero_buf, 0x0, STORAGE_BLOCK_SIZE);
		for (uint32_t i = 0; i < needed_blocks; i++) {
			write_blocks(zero_buf, file->start_block +
				     file->num_blocks + i, 1);
		}

		file->num_blocks += needed_blocks;
		
		return 0;
	} else {
		return ERR_FOUND;
	}
}

static int expand_empty_file(struct file *file, uint32_t needed_blocks)
{
	/* Figure out if we have enough empty blocks to allocate.
	 * We will allocate space only after the last file.
	 */
	uint32_t start_block = DIR_DATA_NUM_BLOCKS;

	for (struct file_list_node *node = file_list_head; node;
	     node = node->next) {
		if (node->file->start_block >= start_block)
				start_block = node->file->start_block +
					node->file->num_blocks;
	}

	if (start_block + needed_blocks >= partition_num_blocks)
		return ERR_FOUND;

	/* zero out the new blocks */
	uint8_t zero_buf[STORAGE_BLOCK_SIZE];
	memset(zero_buf, 0x0, STORAGE_BLOCK_SIZE);
	for (uint32_t i = 0; i < needed_blocks; i++) {
		write_blocks(zero_buf, start_block + i, 1);
	}

	file->start_block = start_block;
	file->num_blocks = needed_blocks;

	return 0;
}

/*
 * @size: the overall size needed for the file to be expanded to.
 */
static int expand_file_size(struct file *file, uint32_t size)
{
	bool empty_file;
	uint32_t needed_size, needed_blocks, leftover;
	int ret = 0;

	if (file->size >= size)
		return 0;

	/* Figure out how many more blocks we need */
	if (file->size == 0) {
		empty_file = true;
		needed_size = size;
	} else {
		empty_file = false;
		needed_size = size - file->size;
	}

	/* first check if there's enough space in the last block */
	leftover = STORAGE_BLOCK_SIZE - (file->size % STORAGE_BLOCK_SIZE);
	if ((leftover != STORAGE_BLOCK_SIZE) && leftover >= needed_size)
		goto update;

	needed_blocks = needed_size / STORAGE_BLOCK_SIZE;
	if (needed_size % STORAGE_BLOCK_SIZE)
		needed_blocks++;

	if (empty_file)
		ret = expand_empty_file(file, needed_blocks);
	else
		ret = expand_existing_file(file, needed_blocks);

	if (!ret) {
update:
		file->size = size;
		ret = update_file_in_directory(file);
		if (ret)
			/* FIXME: the dir is not consistent with the in-memory
			 * file info. */
			printf("Error: %s: couldn't update file info in "
			       "directory.\n", __func__);
		flush_dir_data_to_storage();
	}

	return ret;
}

static void release_file_blocks(struct file *file)
{
	/* No op */
}

uint32_t file_system_open_file(char *filename, uint32_t mode)
{
	struct file *file = NULL;
	if (!(mode == FILE_OPEN_MODE || mode == FILE_OPEN_CREATE_MODE)) {
		printf("Error: invalid mode for opening a file\n");
		return (uint32_t) 0;
	}

	for (struct file_list_node *node = file_list_head; node;
	     node = node->next) {
		if (!strcmp(node->file->filename, filename)) {
			if (node->file->opened)
				/* error */
				return (uint32_t) 0;

			file = node->file;
		}
	}

	if (file == NULL && mode == FILE_OPEN_CREATE_MODE) {
		file = (struct file *) malloc(sizeof(struct file));
		if (!file)
			return (uint32_t) 0;

		strcpy(file->filename, filename);

		file->start_block = 0;
		file->num_blocks = 0;
		file->size = 0;

		int ret = add_file_to_directory(file);
		if (ret) {
			release_file_blocks(file);
			free(file);
			return (uint32_t) 0;
		}

		add_file_to_list(file);
	}

	if (file) {
		int ret = get_unused_fd();
		if (ret < 0)
			return (uint32_t) 0;

		uint32_t fd = (uint32_t) ret;
		if (fd == 0 || fd >= MAX_NUM_FD)
			return (uint32_t) 0;

		/* Shouldn't happen, but let's check. */
		if (file_array[fd])
			return (uint32_t) 0;

		file_array[fd] = file;
		file->opened = true;

		return fd;
	}

	/* error */
	return (uint32_t) 0;
}

/*
 * This API allows growing the file size, but only if there is enough empty
 * blocks right after the last file block in the partition.
 */
uint32_t file_system_write_to_file(uint32_t fd, uint8_t *data, uint32_t size,
				   uint32_t offset)
{
	if (fd == 0 || fd >= MAX_NUM_FD) {
		printf("Error: %s: fd is 0 or too large (%d)\n", __func__, fd);
		return 0;
	}

	struct file *file = file_array[fd];
	if (!file) {
		printf("Error: %s: invalid fd\n", __func__);
		return 0;
	}

	if (!file->opened) {
		printf("Error: %s: file not opened!\n", __func__);
		return 0;
	}

	if (file->size < (offset + size)) {
		if (offset > file->size) {
			printf("Error: %s: invalid offset (offset = %d, "
			       "file->size = %d\n", __func__, offset,
			       file->size);
			return 0;
		}
		/* Try to expand the file size */
		expand_file_size(file, offset + size);
	}

	if (offset >= file->size) {
		return 0;
	}

	/* partial write */
	if (file->size < (offset + size)) {
		size = file->size - offset; 
	}

	uint32_t block_num = offset / STORAGE_BLOCK_SIZE;
	uint32_t block_offset = offset % STORAGE_BLOCK_SIZE;
	uint32_t written_size = 0;
	uint32_t next_write_size = STORAGE_BLOCK_SIZE - block_offset;
	if (next_write_size > size)
		next_write_size = size;
	uint32_t ret = 0;

	while (written_size < size) {
		ret = write_to_block(&data[written_size], file->start_block +
				     block_num, block_offset, next_write_size);
		if (ret != next_write_size) {
			written_size += ret;
			break;
		}
		written_size += next_write_size;
		block_num++;
		block_offset = 0;
		if ((size - written_size) >= STORAGE_BLOCK_SIZE)
			next_write_size = STORAGE_BLOCK_SIZE - block_offset;
		else
			next_write_size = (size - written_size);
	}

	return written_size;
}

uint32_t file_system_read_from_file(uint32_t fd, uint8_t *data, uint32_t size,
				    uint32_t offset)
{
	if (fd == 0 || fd >= MAX_NUM_FD) {
		printf("Error: %s: fd is 0 or too large (%d)\n", __func__, fd);
		return 0;
	}

	struct file *file = file_array[fd];
	if (!file) {
		printf("Error: %s: invalid fd\n", __func__);
		return 0;
	}

	if (!file->opened) {
		printf("Error: %s: file not opened!\n", __func__);
		return 0;
	}

	if (offset >= file->size) {
		return 0;
	}

	/* partial read */
	if (file->size < (offset + size)) {
		size = file->size - offset; 
	}

	uint32_t block_num = offset / STORAGE_BLOCK_SIZE;
	uint32_t block_offset = offset % STORAGE_BLOCK_SIZE;
	uint32_t read_size = 0;
	uint32_t next_read_size = STORAGE_BLOCK_SIZE - block_offset;
	if (next_read_size > size)
		next_read_size = size;
	uint32_t ret = 0;

	while (read_size < size) {
		ret = read_from_block(&data[read_size], file->start_block +
				      block_num, block_offset, next_read_size);
		if (ret != next_read_size) {
			read_size += ret;
			break;
		}
		read_size += next_read_size;
		block_num++;
		block_offset = 0;
		if ((size - read_size) >= STORAGE_BLOCK_SIZE)
			next_read_size = STORAGE_BLOCK_SIZE - block_offset;
		else
			next_read_size = (size - read_size);
	}

	return read_size;
}

int file_system_close_file(uint32_t fd)
{
	if (fd == 0 || fd >= MAX_NUM_FD) {
		printf("Error: %s: fd is 0 or too large (%d)\n", __func__, fd);
		return ERR_INVALID;
	}

	struct file *file = file_array[fd];
	if (!file) {
		printf("Error: %s: invalid fd\n", __func__);
		return ERR_INVALID;
	}

	if (!file->opened) {
		printf("Error: %s: file not opened!\n", __func__);
		return ERR_INVALID;
	}

	file->opened = false;
	file_array[fd] = NULL;
	mark_fd_as_unused(fd);

	return 0;
}

void initialize_file_system(uint32_t _partition_num_blocks)
{
	file_list_head = NULL;
	file_list_tail = NULL;
	dir_data_ptr = 0;
	partition_num_blocks = 0;

	/* initialize fd bitmap */
	if (MAX_NUM_FD % 8) {
		printf("Error: %s: MAX_NUM_FD must be divisible by 8\n",
		       __func__);
		_exit(-1);
	}

	fd_bitmap[0] = 0x00000001; /* fd 0 is error */
	for (int i = 1; i < (MAX_NUM_FD / 8); i++)
		fd_bitmap[i] = 0;

	partition_num_blocks = _partition_num_blocks;

	/* read the directory */
	read_dir_data_from_storage();
	/* check to see if there's a valid directory */
	if (dir_data[0] == '$' && dir_data[1] == '%' &&
	    dir_data[2] == '^' && dir_data[3] == '&') {
		/* retrieve file info */

		uint16_t num_files = *((uint16_t *) &dir_data[4]);
		dir_data_ptr = 6;

		for (int i = 0; i < num_files; i++) {
			int dir_data_off = dir_data_ptr;
			if ((dir_data_ptr + 2) > DIR_DATA_SIZE)
				break;

			int filename_size =
				*((uint16_t *) &dir_data[dir_data_ptr]);

			if ((dir_data_ptr + filename_size + 15) > DIR_DATA_SIZE)
				break;
			dir_data_ptr += 2;

			if (filename_size > MAX_FILENAME_SIZE)
				break;

			struct file *file =
				(struct file *) malloc(sizeof(struct file));
			if (!file)
				break;

			strcpy(file->filename,
			       (char *) &dir_data[dir_data_ptr]);
			dir_data_ptr = dir_data_ptr + filename_size + 1;

			file->dir_data_off = dir_data_off;

			file->start_block =
				*((uint32_t *) &dir_data[dir_data_ptr]);
			dir_data_ptr += 4;
			file->num_blocks =
				*((uint32_t *) &dir_data[dir_data_ptr]);
			dir_data_ptr += 4;
			file->size = *((uint32_t *) &dir_data[dir_data_ptr]);
			dir_data_ptr += 4;
			
			file->opened = 0;
			add_file_to_list(file);
		}
	} else {
		/* initialize signature */
		dir_data[0] = '$';
		dir_data[1] = '%';
		dir_data[2] = '^';
		dir_data[3] = '&';
		/* set num files (two bytes) to 0 */
		dir_data[4] = 0;
		dir_data[5] = 0;
		dir_data_ptr = 6;
		/* update the directory in storage */
		flush_dir_data_to_storage();
	}

	for (int i = 0; i < MAX_NUM_FD; i++)
		file_array[i] = NULL;
}

void close_file_system(void)
{
	/* Not currently useful as we flush on every update. */
	flush_dir_data_to_storage();
}