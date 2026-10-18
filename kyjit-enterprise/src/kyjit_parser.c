#include <immintrin.h>
#include "kyjit_parser.h"

// Branchless number parsing using SIMD digit detection
long long kyjit_parse_numbers_branchless(const char *s, int len) {
    long long total = 0;
    int i = 0;
    for(; i+32 <= len; i+=32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&s[i]);
        __m256i zero = _mm256_set1_epi8('0');
        __m256i nine = _mm256_set1_epi8('9');
        __m256i is_digit = _mm256_and_si256(
            _mm256_cmpgt_epi8(v, _mm256_sub_epi8(zero, _mm256_set1_epi8(1))),
            _mm256_cmpgt_epi8(_mm256_add_epi8(nine, _mm256_set1_epi8(1)), v));
        int mask = _mm256_movemask_epi8(is_digit);
        if(mask) {
            for(int j=0;j<32;j++) {
                if(mask & (1<<j)) {
                    long long num = 0;
                    int k = i+j;
                    while(k < len && s[k] >= '0' && s[k] <= '9') {
                        num = num*10 + (s[k]-'0');
                        k++;
                    }
                    total += num;
                    i = k-1;
                    break;
                }
            }
        }
    }
    for(; i<len; i++) {
        if(s[i] >= '0' && s[i] <= '9') {
            long long num = 0;
            while(i < len && s[i] >= '0' && s[i] <= '9') {
                num = num*10 + (s[i]-'0');
                i++;
            }
            total += num;
        }
    }
    return total;
}
