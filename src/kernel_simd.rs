#![allow(unsafe_op_in_unsafe_fn)]

use std::arch::x86_64::*;
use crate::kernel::{
    INITIAL_ARR, RangeResult, SEED_STRIDE, materialize_arr, xseed,
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

/// computes `n % D` for 8 packed 32-bit integers since avx2 doesn't have a native modulo operation
/// uses the algorithm found in "Division by Invariant Integers using Multiplication" by T. Granlund and P. L. Montgomery.
#[inline(always)]
pub unsafe fn fast_mod_avx2<const D: u32>(a: __m256i) -> __m256i 
    where
        [(); { crate::libdivide::magic_m::<{ D }>() } as usize]:,
        [(); { crate::libdivide::magic_sh1::<{ D }>() } as usize]:,
        [(); { crate::libdivide::magic_sh2::<{ D }>() } as usize]:
{
    let m1 = _mm256_set1_epi32(crate::libdivide::magic_m::<{ D }>());
    let d1 = _mm256_set1_epi32(D as i32);

    // even
    let t1 = _mm256_mul_epu32(a, m1);
    let t2 = _mm256_srli_epi64::<32>(t1);
    // odd
    let t3 = _mm256_srli_epi64::<32>(a);
    let t4 = _mm256_mul_epu32(t3, m1);

    let mask = _mm256_set_epi32(-1, 0, -1, 0, -1, 0, -1, 0);
    let t7 = _mm256_blendv_epi8(t2, t4, mask);

    let t8 = _mm256_sub_epi32(a, t7);
    let t9 = _mm256_srli_epi32::<{ crate::libdivide::magic_sh1::<{ D }>() }>(t8);
    let t10 = _mm256_add_epi32(t7, t9);
    let t11 = _mm256_srli_epi32::<{ crate::libdivide::magic_sh2::<{ D }>() }>(t10);

    let t12 = _mm256_mullo_epi32(t11, d1);
    _mm256_sub_epi32(a, t12)
}

/// we explicitly don't do rejection sampling because the cold path is exceptionally rare
/// (<1 in 10^9) and we're okay theoretically missing a few true positives since we filter
/// later.
#[inline(always)]
unsafe fn xint8<const MAX: u32>(rng: &mut Xoshiro8) -> __m256i
    where 
        [(); { crate::libdivide::magic_m::<{ MAX }>() } as usize]:,
        [(); { crate::libdivide::magic_sh1::<{ MAX }>() } as usize]:,
        [(); { crate::libdivide::magic_sh2::<{ MAX }>() } as usize]: {
    fast_mod_avx2::<MAX>(xnext8(rng))
}

#[inline(always)]
unsafe fn init_rng8(seed: u64, base_index: u64) -> Xoshiro8 {
    let mut s0 = [0u32; 8];
    let mut s1 = [0u32; 8];
    let mut s2 = [0u32; 8];
    let mut s3 = [0u32; 8];

    // this is always faster in scalar, no matter how hard i try to vectorize it
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

unsafe fn score8(rng: &mut Xoshiro8) -> __m256i {
    let mut foreign_hits = _mm256_setzero_si256();
    let mut fixed = _mm256_setzero_si256();
    let ones = _mm256_set1_epi32(1);
    let zeros = _mm256_setzero_si256();

    macro_rules! step {
        ($idx:expr, $max:expr) => {{
            let draw = xint8::<$max>(rng);

            let idx_bit = _mm256_set1_epi32(1i32 << $idx);

            let zero_mask = _mm256_cmpeq_epi32(_mm256_and_si256(foreign_hits, idx_bit), zeros);
            let fixed_if = _mm256_add_epi32(fixed, _mm256_and_si256(zero_mask, ones));

            let bit = _mm256_sllv_epi32(ones, draw);
            let foreign_hits_else = _mm256_or_si256(foreign_hits, bit);

            let mask = _mm256_cmpeq_epi32(draw, _mm256_set1_epi32($idx as i32)); // draw == $idx
            fixed = _mm256_blendv_epi8(fixed, fixed_if, mask);
            foreign_hits = _mm256_blendv_epi8(foreign_hits_else, foreign_hits, mask);
        }};
    }

    step!(24, 25);
    step!(23, 24);
    step!(22, 23);
    step!(21, 22);
    step!(20, 21);
    step!(19, 20);
    step!(18, 19);
    step!(17, 18);
    step!(16, 17);
    step!(15, 16);
    step!(14, 15);
    step!(13, 14);
    step!(12, 13);
    step!(11, 12);
    step!(10, 11);
    step!(9,  10);
    step!(8,  9);
    step!(7,  8);
    step!(6,  7);
    step!(5,  6);
    step!(4,  5);
    step!(3,  4);
    step!(2,  3);
    step!(1,  2);

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

    let mut best_score: u8 = 0;
    let mut best_index = lo;

    let mut i = lo;

    while i + 8 <= hi {
        let mut rng = init_rng8(seed, i);
        let scores = score8(&mut rng);

        let mut buf = [0i32; 8];
        _mm256_storeu_si256(buf.as_mut_ptr() as *mut __m256i, scores);

        for lane in 0..8 {
            if buf[lane] as u8 > best_score {
                // re-evaluate using scalar implementation because of rejection sampling
                let arr = materialize_arr(seed, i + lane as u64);
                let score = crate::kernel::count_fixed_points(&arr);

                if score > best_score {
                    best_score = score;
                    best_index = i + lane as u64;
                }
            }
        }

        i += 8;
    }

    // tail (scalar fallback)
    while i < hi {
        let arr = materialize_arr(seed, i);
        let score = crate::kernel::count_fixed_points(&arr);

        if score > best_score {
            best_score = score;
            best_index = i;
        }

        i += 1;
    }

    let best_arr = materialize_arr(seed, best_index);

    RangeResult {
        best_score,
        best_arr,
        best_index,
    }
}

#[cfg(test)]
mod tests {
    use std::arch::x86_64::*;
    use crate::kernel_simd::fast_mod_avx2;

    #[test]
    fn simd_mod_const_works() {
        // test various inputs mod 5
        let inputs: [(u32, u32); _] = [
            (25, 0),
            (24, 4),
            (23, 3),
            (22, 2),
            (21, 1),
            (101, 1)
        ];
        // scalar obvious
        for (a, expected) in inputs {
            assert_eq!(a % 5, expected);
        }
        // simd
        for (a, expected) in inputs {
            unsafe {
                let avec = _mm256_set1_epi32(a as i32);
                let expectedvec = _mm256_set1_epi32(expected as i32);
                let result = fast_mod_avx2::<5>(avec);
    
                let mut resultarr = [0i32; 8];
                _mm256_storeu_si256(resultarr.as_mut_ptr() as *mut __m256i, result);
                let mut expectedarr = [expected as i32; 8];
                _mm256_storeu_si256(expectedarr.as_mut_ptr() as *mut __m256i, expectedvec);
                assert_eq!(resultarr, expectedarr);
            }
        }
    }
}