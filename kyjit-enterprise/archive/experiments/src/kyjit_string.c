#include <immintrin.h>
#include <stdint.h>
#include <string.h>
#include "kyjit_string.h"

int kyjit_string_lowercase_avx2(const char *str, size_t len) {
    int count = 0;
    size_t i = 0;
    
    for(; i+512 <= len; i+=512) {
        __builtin_prefetch(&str[i+1024], 0, 3);
        __m256i lo = _mm256_set1_epi8('a'-1);
        __m256i hi = _mm256_set1_epi8('z'+1);
        
        __m256i v0 = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&str[i+32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&str[i+64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&str[i+96]);
        __m256i v4 = _mm256_loadu_si256((__m256i*)&str[i+128]);
        __m256i v5 = _mm256_loadu_si256((__m256i*)&str[i+160]);
        __m256i v6 = _mm256_loadu_si256((__m256i*)&str[i+192]);
        __m256i v7 = _mm256_loadu_si256((__m256i*)&str[i+224]);
        __m256i v8 = _mm256_loadu_si256((__m256i*)&str[i+256]);
        __m256i v9 = _mm256_loadu_si256((__m256i*)&str[i+288]);
        __m256i v10 = _mm256_loadu_si256((__m256i*)&str[i+320]);
        __m256i v11 = _mm256_loadu_si256((__m256i*)&str[i+352]);
        __m256i v12 = _mm256_loadu_si256((__m256i*)&str[i+384]);
        __m256i v13 = _mm256_loadu_si256((__m256i*)&str[i+416]);
        __m256i v14 = _mm256_loadu_si256((__m256i*)&str[i+448]);
        __m256i v15 = _mm256_loadu_si256((__m256i*)&str[i+480]);
        
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v0,lo),_mm256_cmpgt_epi8(hi,v0))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v1,lo),_mm256_cmpgt_epi8(hi,v1))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v2,lo),_mm256_cmpgt_epi8(hi,v2))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v3,lo),_mm256_cmpgt_epi8(hi,v3))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v4,lo),_mm256_cmpgt_epi8(hi,v4))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v5,lo),_mm256_cmpgt_epi8(hi,v5))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v6,lo),_mm256_cmpgt_epi8(hi,v6))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v7,lo),_mm256_cmpgt_epi8(hi,v7))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v8,lo),_mm256_cmpgt_epi8(hi,v8))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v9,lo),_mm256_cmpgt_epi8(hi,v9))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v10,lo),_mm256_cmpgt_epi8(hi,v10))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v11,lo),_mm256_cmpgt_epi8(hi,v11))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v12,lo),_mm256_cmpgt_epi8(hi,v12))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v13,lo),_mm256_cmpgt_epi8(hi,v13))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v14,lo),_mm256_cmpgt_epi8(hi,v14))));
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v15,lo),_mm256_cmpgt_epi8(hi,v15))));
    }
    
    for(; i+32 <= len; i+=32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i lo = _mm256_set1_epi8('a'-1);
        __m256i hi = _mm256_set1_epi8('z'+1);
        count += __builtin_popcount(_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpgt_epi8(v,lo),_mm256_cmpgt_epi8(hi,v))));
    }
    
    for(; i<len; i++) {
        if(str[i] >= 'a' && str[i] <= 'z') count++;
    }
    return count;
}
