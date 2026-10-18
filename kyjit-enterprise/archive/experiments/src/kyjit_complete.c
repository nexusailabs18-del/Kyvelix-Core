#include "kyjit_complete.h"
#include <stdlib.h>
#include <string.h>
#include <immintrin.h>
#include <cblas.h>
#include <time.h>

uint64_t kyjit_fib_safe(int n) {
    uint64_t a=0,b=1;
    for(int i=0;i<n;i++){
        uint64_t t=(a+b)%1000000007;
        a=b;b=t;
    }
    return a;
}

int kyjit_sieve_safe(int n) {
    if(n > 10000000) n = 10000000;
    char *p = calloc(n+1,1);
    if(!p) return -1;
    int c=0;
    for(int i=2;i<=n;i++){
        if(!p[i]){
            c++;
            for(int j=i*2;j<=n;j+=i)p[j]=1;
        }
    }
    free(p);
    return c;
}

void kyjit_matmul_openblas(float *A, float *B, float *C, int N) {
    cblas_sgemm(CblasRowMajor,CblasNoTrans,CblasNoTrans,
                N,N,N,1.0f,A,N,B,N,0.0f,C,N);
}

long long kyjit_mfr_simd(int32_t *data, int n) {
    long long sum=0;
    int i=0;
    for(;i+32<=n;i+=32){
        __m256i v0=_mm256_loadu_si256((__m256i*)&data[i]);
        __m256i v1=_mm256_loadu_si256((__m256i*)&data[i+8]);
        __m256i v2=_mm256_loadu_si256((__m256i*)&data[i+16]);
        __m256i v3=_mm256_loadu_si256((__m256i*)&data[i+24]);
        __m256i d0=_mm256_slli_epi32(v0,1);
        __m256i d1=_mm256_slli_epi32(v1,1);
        __m256i d2=_mm256_slli_epi32(v2,1);
        __m256i d3=_mm256_slli_epi32(v3,1);
        
        /* Manual modulo 3: d - (d*0x55555556 >> 30) * 3 */
        __m256i magic = _mm256_set1_epi32(0x55555556);
        __m256i q0 = _mm256_srli_epi32(_mm256_mullo_epi32(d0, magic), 30);
        __m256i q1 = _mm256_srli_epi32(_mm256_mullo_epi32(d1, magic), 30);
        __m256i q2 = _mm256_srli_epi32(_mm256_mullo_epi32(d2, magic), 30);
        __m256i q3 = _mm256_srli_epi32(_mm256_mullo_epi32(d3, magic), 30);
        
        __m256i three = _mm256_set1_epi32(3);
        __m256i mod0 = _mm256_sub_epi32(d0, _mm256_mullo_epi32(q0, three));
        __m256i mod1 = _mm256_sub_epi32(d1, _mm256_mullo_epi32(q1, three));
        __m256i mod2 = _mm256_sub_epi32(d2, _mm256_mullo_epi32(q2, three));
        __m256i mod3 = _mm256_sub_epi32(d3, _mm256_mullo_epi32(q3, three));
        
        __m256i zero = _mm256_setzero_si256();
        __m256i m0 = _mm256_cmpeq_epi32(mod0, zero);
        __m256i m1 = _mm256_cmpeq_epi32(mod1, zero);
        __m256i m2 = _mm256_cmpeq_epi32(mod2, zero);
        __m256i m3 = _mm256_cmpeq_epi32(mod3, zero);
        
        __m256i r0 = _mm256_and_si256(d0, m0);
        __m256i r1 = _mm256_and_si256(d1, m1);
        __m256i r2 = _mm256_and_si256(d2, m2);
        __m256i r3 = _mm256_and_si256(d3, m3);
        
        __m256i sumv = _mm256_add_epi32(
            _mm256_add_epi32(r0,r1),
            _mm256_add_epi32(r2,r3));
        
        int tmp[8];
        _mm256_storeu_si256((__m256i*)tmp, sumv);
        sum += tmp[0]+tmp[1]+tmp[2]+tmp[3]+tmp[4]+tmp[5]+tmp[6]+tmp[7];
    }
    for(;i<n;i++){int v=data[i]*2;if(v%3==0)sum+=v;}
    return sum;
}

long long kyjit_json_parse(const char *json, int len) {
    long long total=0;
    for(int i=0;i<len;i++){
        if(json[i]>='0'&&json[i]<='9'){
            long long num=0;
            while(i<len&&json[i]>='0'&&json[i]<='9'){num=num*10+(json[i]-'0');i++;}
            total+=num;
        }
    }
    return total;
}

int kyjit_string_lower(const char *str, int len) {
    int count=0;
    int i=0;
    for(;i+512<=len;i+=512){
        __m256i lo=_mm256_set1_epi8('a'-1);
        __m256i hi=_mm256_set1_epi8('z'+1);
        for(int j=0;j<512;j+=32){
            __m256i v=_mm256_loadu_si256((__m256i*)&str[i+j]);
            count+=__builtin_popcount(_mm256_movemask_epi8(
                _mm256_and_si256(_mm256_cmpgt_epi8(v,lo),_mm256_cmpgt_epi8(hi,v))));
        }
    }
    for(;i<len;i++){if(str[i]>='a'&&str[i]<='z')count++;}
    return count;
}

long long kyjit_hash_lookup(int *table, int size, int *keys, int n) {
    long long sum=0;
    for(int i=0;i<n;i++)sum+=table[keys[i]%size];
    return sum;
}
