#![allow(unsafe_op_in_unsafe_fn)]

use std::arch::x86_64::*;
use crate::kernel::{
    INITIAL_ARR, RangeResult, SEED_STRIDE, THRESHOLD_2, THRESHOLD_3, THRESHOLD_4, THRESHOLD_5, THRESHOLD_6, THRESHOLD_7, THRESHOLD_8, THRESHOLD_9, THRESHOLD_10, THRESHOLD_11, THRESHOLD_12, THRESHOLD_13, THRESHOLD_14, THRESHOLD_15, THRESHOLD_16, THRESHOLD_17, THRESHOLD_18, THRESHOLD_19, THRESHOLD_20, THRESHOLD_21, THRESHOLD_22, THRESHOLD_23, THRESHOLD_24, THRESHOLD_25, materialize_arr, xseed,
};

#[repr(C, align(32))]
#[derive(Copy, Clone)]
struct Xoshiro8 {
    s0: __m256i,
    s1: __m256i,
    s2: __m256i,
    s3: __m256i,
}

#[inline(always)]
unsafe fn rotl32<const K: i32>(x: __m256i) -> __m256i
    where [(); { 32 - K } as usize]: {
    _mm256_or_si256(
        _mm256_slli_epi32::<K>(x),
        _mm256_srli_epi32::<{ 32 - K }>(x),
    )
}

#[inline(always)]
unsafe fn xnext8(rng: &mut Xoshiro8) -> __m256i {
    let sum = _mm256_add_epi32(rng.s0, rng.s3);
    let res = _mm256_add_epi32(rotl32::<7>(sum), rng.s0);

    let t = _mm256_slli_epi32::<9>(rng.s1);

    rng.s2 = _mm256_xor_si256(rng.s2, rng.s0);
    rng.s3 = _mm256_xor_si256(rng.s3, rng.s1);
    rng.s1 = _mm256_xor_si256(rng.s1, rng.s2);
    rng.s0 = _mm256_xor_si256(rng.s0, rng.s3);

    rng.s2 = _mm256_xor_si256(rng.s2, t);
    rng.s3 = rotl32::<11>(rng.s3);

    res
}

/// Vectorized rejection sampling and compile-time constant modulo using AVX2.
#[inline(always)]
unsafe fn xint8<const MAX: u32>(rng: &mut Xoshiro8, threshold: u32) -> __m256i {
    let v_threshold = _mm256_set1_epi32(threshold as i32);
    
    // mask tracking which lanes still need a valid value (all 1s initially)
    let mut active_mask = _mm256_set1_epi32(-1);
    let mut results = _mm256_setzero_si256();

    loop {
        let values = xnext8(rng); 

        // unsigned values >= threshold
        // avx2 doesn't have unsigned comparison operators, so we flip the msb to
        // map it to the signed range. seems like this is the standard approach?
        let sign_mask = _mm256_set1_epi32(i32::MIN);
        let v_shifted = _mm256_xor_si256(values, sign_mask);
        let t_shifted = _mm256_xor_si256(v_threshold, sign_mask);
        
        let passed_gt = _mm256_cmpgt_epi32(v_shifted, t_shifted);
        let passed_eq = _mm256_cmpeq_epi32(values, v_threshold);
        let ge_mask = _mm256_or_si256(passed_gt, passed_eq);

        // lanes that just passed and still need their slot filled
        let ready_mask = _mm256_and_si256(ge_mask, active_mask);

        results = _mm256_blendv_epi8(results, values, ready_mask);
        active_mask = _mm256_andnot_si256(ready_mask, active_mask);

        // check if all 8 lanes are satisfied (active_mask is entirely 0)
        if _mm256_testz_si256(active_mask, active_mask) == 1 {
            break;
        }
    }

    // avx2 doesn't have a modulo operation, so we fall back to scalar
    let mut lanes = [0u32; 8];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, results);
    for lane in &mut lanes {
        *lane %= MAX;
    }
    _mm256_loadu_si256(lanes.as_ptr() as *const __m256i)
}

#[inline(always)]
unsafe fn init_rng8(seed: u64, base_index: u64) -> Xoshiro8 {
    let mut s0 = [0u32; 8];
    let mut s1 = [0u32; 8];
    let mut s2 = [0u32; 8];
    let mut s3 = [0u32; 8];

    for i in 0..8 {
        let seed_i = seed.wrapping_add((base_index + i as u64).wrapping_mul(SEED_STRIDE));

        let st = xseed(seed_i);

        s0[i] = st[0];
        s1[i] = st[1];
        s2[i] = st[2];
        s3[i] = st[3];
    }

    Xoshiro8 {
        s0: _mm256_loadu_si256(s0.as_ptr() as *const __m256i),
        s1: _mm256_loadu_si256(s1.as_ptr() as *const __m256i),
        s2: _mm256_loadu_si256(s2.as_ptr() as *const __m256i),
        s3: _mm256_loadu_si256(s3.as_ptr() as *const __m256i),
    }
}

/// SIMD scoring kernel: 8 candidates in lockstep
#[inline(always)]
unsafe fn score8(rng: &mut Xoshiro8) -> __m256i {
    let mut foreign_hits = _mm256_setzero_si256();
    let mut fixed = _mm256_setzero_si256();
    let ones = _mm256_set1_epi32(1);

    macro_rules! step {
        ($idx:expr, $max:expr, $threshold:expr) => {{
            let draw = xint8::<$max>(rng, $threshold);

            let idx = _mm256_set1_epi32($idx as i32);

            let idx_bit = _mm256_sllv_epi32(ones, idx);
            let zero_mask = _mm256_cmpeq_epi32(_mm256_and_si256(foreign_hits, idx_bit), _mm256_setzero_si256());
            let fixed_if = _mm256_add_epi32(fixed, _mm256_and_si256(zero_mask, ones));

            let bit = _mm256_sllv_epi32(ones, draw);
            let foreign_hits_else = _mm256_or_si256(foreign_hits, bit);

            let mask = _mm256_cmpeq_epi32(draw, idx); // draw == $idx
            fixed = _mm256_blendv_epi8(fixed, fixed_if, mask);
            foreign_hits = _mm256_blendv_epi8(foreign_hits_else, foreign_hits, mask);
        }};
    }

    step!(24, 25, THRESHOLD_25);
    step!(23, 24, THRESHOLD_24);
    step!(22, 23, THRESHOLD_23);
    step!(21, 22, THRESHOLD_22);
    step!(20, 21, THRESHOLD_21);
    step!(19, 20, THRESHOLD_20);
    step!(18, 19, THRESHOLD_19);
    step!(17, 18, THRESHOLD_18);
    step!(16, 17, THRESHOLD_17);
    step!(15, 16, THRESHOLD_16);
    step!(14, 15, THRESHOLD_15);
    step!(13, 14, THRESHOLD_14);
    step!(12, 13, THRESHOLD_13);
    step!(11, 12, THRESHOLD_12);
    step!(10, 11, THRESHOLD_11);
    step!(9,  10, THRESHOLD_10);
    step!(8,  9,  THRESHOLD_9);
    step!(7,  8,  THRESHOLD_8);
    step!(6,  7,  THRESHOLD_7);
    step!(5,  6,  THRESHOLD_6);
    step!(4,  5,  THRESHOLD_5);
    step!(3,  4,  THRESHOLD_4);
    step!(2,  3,  THRESHOLD_3);
    step!(1,  2,  THRESHOLD_2);

    // fixed + ((foreign_hits & 1) == 0) as u8
    let mask = _mm256_and_si256(foreign_hits, ones);
    let cmp = _mm256_cmpeq_epi32(mask, _mm256_setzero_si256());
    let add = _mm256_and_si256(cmp, ones);
    _mm256_add_epi32(fixed, add)
}

#[target_feature(enable = "avx2")]
pub unsafe fn run_range_simd(seed: u64, lo: u64, hi: u64) -> RangeResult {
    if lo >= hi {
        return RangeResult {
            best_score: 0,
            best_arr: INITIAL_ARR,
            best_index: lo,
        };
    }

    let mut best_score: i32 = -1;
    let mut best_index = lo;

    let mut i = lo;

    while i + 8 <= hi {
        let mut rng = init_rng8(seed, i);
        let scores = score8(&mut rng);

        let mut buf = [0i32; 8];
        _mm256_storeu_si256(buf.as_mut_ptr() as *mut __m256i, scores);

        for lane in 0..8 {
            if buf[lane] > best_score {
                best_score = buf[lane];
                best_index = i + lane as u64;
            }
        }

        i += 8;
    }

    // tail (scalar fallback)
    let mut best_score_u8 = best_score.max(0) as u8;
    while i < hi {
        let arr = materialize_arr(seed, i);

        let score = crate::kernel::count_fixed_points(&arr);

        if score > best_score_u8 {
            best_score_u8 = score;
            best_index = i;
        }

        i += 1;
    }

    let best_arr = materialize_arr(seed, best_index);

    RangeResult {
        best_score: best_score_u8,
        best_arr,
        best_index,
    }
}