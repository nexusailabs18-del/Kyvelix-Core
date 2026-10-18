#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <stdlib.h>
#include "ky_codec.h"

int main() {
    printf("=== Test Codec ===\n");
    
    uint8_t buffer[256];
    ky_codec_ctx_t writer;
    ky_codec_init_writer(&writer, buffer, sizeof(buffer));
    
    /* Write values */
    assert(ky_codec_write_u8(&writer, 42) == KY_CODEC_OK);
    assert(ky_codec_write_u16(&writer, 12345) == KY_CODEC_OK);
    assert(ky_codec_write_u32(&writer, 987654321) == KY_CODEC_OK);
    assert(ky_codec_write_string(&writer, "hello") == KY_CODEC_OK);
    
    const char *strs[] = {"one", "two", "three"};
    assert(ky_codec_write_string_array(&writer, strs, 3) == KY_CODEC_OK);
    
    printf("✅ Codec write passed\n");
    
    /* Read values back */
    ky_codec_ctx_t reader;
    ky_codec_init_reader(&reader, buffer, ky_codec_written(&writer));
    
    uint8_t u8;
    uint16_t u16;
    uint32_t u32;
    
    assert(ky_codec_read_u8(&reader, &u8) == KY_CODEC_OK);
    assert(u8 == 42);
    
    assert(ky_codec_read_u16(&reader, &u16) == KY_CODEC_OK);
    assert(u16 == 12345);
    
    assert(ky_codec_read_u32(&reader, &u32) == KY_CODEC_OK);
    assert(u32 == 987654321);
    
    const char *str;
    size_t str_len;
    assert(ky_codec_read_string(&reader, &str, &str_len) == KY_CODEC_OK);
    assert(str_len == 5);
    assert(memcmp(str, "hello", 5) == 0);
    
    char **str_array;
    size_t *lengths;
    size_t count;
    assert(ky_codec_read_string_array(&reader, &str_array, &lengths, &count) == KY_CODEC_OK);
    assert(count == 3);
    assert(strcmp(str_array[0], "one") == 0);
    assert(strcmp(str_array[1], "two") == 0);
    assert(strcmp(str_array[2], "three") == 0);
    
    /* Cleanup */
    for (size_t i = 0; i < count; i++) {
        free(str_array[i]);
    }
    free(str_array);
    free(lengths);
    
    printf("✅ Codec read passed\n");
    printf("=== Codec Tests Passed ===\n");
    return 0;
}
