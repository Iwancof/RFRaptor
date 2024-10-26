#include <immintrin.h>

void dotprod_8(const int *restrict const r, const int *restrict const i, const int *restrict const h, int *restrict const out) {
  //  r, i, h: pointer to arrays which length is 8
  //  out: pointer to an array which length is 2

  __m256i vec_r = _mm256_loadu_si256((__m256i *)r);
  __m256i vec_i = _mm256_loadu_si256((__m256i *)i);

  __m256i vec_h = _mm256_loadu_si256((__m256i *)h);

  __m256i vec_rh = _mm256_mullo_epi32(vec_r, vec_h);
  __m256i vec_ih = _mm256_mullo_epi32(vec_i, vec_h);

  int r_result[8], i_result[8];
  _mm256_storeu_si256((__m256i *)r_result, vec_rh);
  _mm256_storeu_si256((__m256i *)i_result, vec_ih);

  int r_sum = r_result[0] + r_result[1] + r_result[2] + r_result[3] + r_result[4] + r_result[5] + r_result[6] + r_result[7];
  int i_sum = i_result[0] + i_result[1] + i_result[2] + i_result[3] + i_result[4] + i_result[5] + i_result[6] + i_result[7];

  out[0] = r_sum;
  out[1] = i_sum;
}

void dotprod_8_horiz(const int *restrict const r, const int *restrict const i, const int *restrict const h, int *restrict const out) {
    // Load vectors
    __m256i vec_r = _mm256_loadu_si256((__m256i *)r);
    __m256i vec_i = _mm256_loadu_si256((__m256i *)i);
    __m256i vec_h = _mm256_loadu_si256((__m256i *)h);

    // Perform element-wise multiplication
    __m256i vec_rh = _mm256_mullo_epi32(vec_r, vec_h);
    __m256i vec_ih = _mm256_mullo_epi32(vec_i, vec_h);

    // Horizontal add in 256-bit registers
    __m256i rh_sum = _mm256_hadd_epi32(vec_rh, vec_rh);
    __m256i ih_sum = _mm256_hadd_epi32(vec_ih, vec_ih);

    // Sum across 128-bit lanes
    rh_sum = _mm256_hadd_epi32(rh_sum, rh_sum);
    ih_sum = _mm256_hadd_epi32(ih_sum, ih_sum);

    // Extract results from 128-bit halves
    out[0] = _mm_extract_epi32(_mm256_castsi256_si128(rh_sum), 0) + _mm_extract_epi32(_mm256_extracti128_si256(rh_sum, 1), 0);
    out[1] = _mm_extract_epi32(_mm256_castsi256_si128(ih_sum), 0) + _mm_extract_epi32(_mm256_extracti128_si256(ih_sum, 1), 0);
}
