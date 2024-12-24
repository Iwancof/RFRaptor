#include <immintrin.h>

void dotprod_8(const int *restrict const r, const int *restrict const i,
               const int *restrict const h, int *restrict const out) {
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

  int r_sum = r_result[0] + r_result[1] + r_result[2] + r_result[3] +
              r_result[4] + r_result[5] + r_result[6] + r_result[7];
  int i_sum = i_result[0] + i_result[1] + i_result[2] + i_result[3] +
              i_result[4] + i_result[5] + i_result[6] + i_result[7];

  out[0] = r_sum;
  out[1] = i_sum;
}

const float scale = 1 / 32768.0;
void dotprod_8_float(const int *restrict const r, const int *restrict const i,
                     const int *restrict const h, float *restrict const out) {
  //  r, i, h: pointer to arrays which length is 8
  //  out: pointer to an array which length is 2

  // Load integer data
  __m256i vec_r = _mm256_loadu_si256((__m256i *)r);
  __m256i vec_i = _mm256_loadu_si256((__m256i *)i);
  __m256i vec_h = _mm256_loadu_si256((__m256i *)h);

  // Multiply integers
  __m256i vec_rh = _mm256_mullo_epi32(vec_r, vec_h);
  __m256i vec_ih = _mm256_mullo_epi32(vec_i, vec_h);

  // Right shift the result by 8 bits
  __m256i vec_rh_shifted = _mm256_srai_epi32(vec_rh, 8);
  __m256i vec_ih_shifted = _mm256_srai_epi32(vec_ih, 8);

  // Horizontal addition for vec_rh_shifted
  __m128i low_rh = _mm256_castsi256_si128(vec_rh_shifted);
  __m128i high_rh = _mm256_extracti128_si256(vec_rh_shifted, 1);
  __m128i sum_rh = _mm_add_epi32(low_rh, high_rh);
  sum_rh = _mm_hadd_epi32(sum_rh, sum_rh);
  sum_rh = _mm_hadd_epi32(sum_rh, sum_rh);
  int r_sum = _mm_cvtsi128_si32(sum_rh);

  // Horizontal addition for vec_ih_shifted (修正箇所)
  __m128i low_ih = _mm256_castsi256_si128(vec_ih_shifted);
  __m128i high_ih = _mm256_extracti128_si256(vec_ih_shifted, 1);
  __m128i sum_ih = _mm_add_epi32(low_ih, high_ih);
  sum_ih = _mm_hadd_epi32(sum_ih, sum_ih);
  sum_ih = _mm_hadd_epi32(sum_ih, sum_ih);
  int i_sum = _mm_cvtsi128_si32(sum_ih);

  // Convert to float and scale
  out[0] = r_sum * scale;
  out[1] = i_sum * scale;
}

void dotprod_8_horiz(const int *restrict const r, const int *restrict const i,
                     const int *restrict const h, int *restrict const out) {
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
  out[0] = _mm_extract_epi32(_mm256_castsi256_si128(rh_sum), 0) +
           _mm_extract_epi32(_mm256_extracti128_si256(rh_sum, 1), 0);
  out[1] = _mm_extract_epi32(_mm256_castsi256_si128(ih_sum), 0) +
           _mm_extract_epi32(_mm256_extracti128_si256(ih_sum, 1), 0);
}
