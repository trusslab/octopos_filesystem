#include "file_system.h"

#define STORAGE_BOOT_PARTITION_SIZE			200000

static void write_file(char* file_name, char* data, uint32_t data_len) {
	uint32_t fd = file_system_open_file(file_name, FILE_OPEN_CREATE_MODE);
	if (fd == 0) {
		printf("Failed to open/create file\n");
		return;
	}

	if (file_system_write_to_file(fd, data, data_len, 0) != data_len) {
		printf("Failed to write everything to file\n");
	}

	if (file_system_close_file(fd) != 0) {
		printf("Failed to close file\n");
	}
}

// cmp_buffer must be at least data_len in size
static void assert_file_eq(char* file_name, char* data, uint32_t data_len, char* cmp_buffer) {
	uint32_t fd = file_system_open_file(file_name, FILE_OPEN_MODE);
	if (fd == 0) {
		printf("Failed to open file\n");
		return;
	}

	if (file_system_read_from_file(fd, cmp_buffer, data_len, 0) != data_len) {
		printf("Failed to read everything from file\n");
	}

	if (file_system_close_file(fd) != 0) {
		printf("Failed to close file\n");
	}

	if (memcmp(data, cmp_buffer, data_len)) {
		printf("File data was incorrect\n");
	}
}

static void test_fs(void)
{
	initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);

	char* text = "This is text in hello";
	write_file("hello", text, strlen(text));

	char* random_text = "aljksdjfalskdfja;slkdfja;s";
	write_file("random", random_text, strlen(random_text));

	char* testing_text = "TESTING TESTING";
	write_file("testing", testing_text, strlen(testing_text));

	char* not_testing_text = "No testing";
	write_file("not_testing", not_testing_text, strlen(not_testing_text));

	char file_cmp_buff[500];
	assert_file_eq("hello", text, strlen(text), file_cmp_buff);

	assert_file_eq("random", random_text, strlen(random_text), file_cmp_buff);

	assert_file_eq("testing", testing_text, strlen(testing_text), file_cmp_buff);

	assert_file_eq("not_testing", not_testing_text, strlen(not_testing_text), file_cmp_buff);

	close_file_system();

	initialize_file_system(STORAGE_BOOT_PARTITION_SIZE);

	assert_file_eq("hello", text, strlen(text), file_cmp_buff);

	assert_file_eq("random", random_text, strlen(random_text), file_cmp_buff);

	assert_file_eq("testing", testing_text, strlen(testing_text), file_cmp_buff);

	assert_file_eq("not_testing", not_testing_text, strlen(not_testing_text), file_cmp_buff);
}


int main(int argc, char **argv)
{
	test_fs();	

	return 0;
}	