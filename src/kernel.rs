pub const N: usize = 25;
pub const SEED_STRIDE: u64 = 0x9e37_79b9_7f4a_7c15;
const LOW_BIT_MASK: u32 = (1u32 << N) - 1;

const INITIAL_ARR: [u8; N] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25,
];

const THRESHOLD_2: u32 = threshold(2);
const THRESHOLD_3: u32 = threshold(3);
const THRESHOLD_4: u32 = threshold(4);
const THRESHOLD_5: u32 = threshold(5);
const THRESHOLD_6: u32 = threshold(6);
const THRESHOLD_7: u32 = threshold(7);
const THRESHOLD_8: u32 = threshold(8);
const THRESHOLD_9: u32 = threshold(9);
const THRESHOLD_10: u32 = threshold(10);
const THRESHOLD_11: u32 = threshold(11);
const THRESHOLD_12: u32 = threshold(12);
const THRESHOLD_13: u32 = threshold(13);
const THRESHOLD_14: u32 = threshold(14);
const THRESHOLD_15: u32 = threshold(15);
const THRESHOLD_16: u32 = threshold(16);
const THRESHOLD_17: u32 = threshold(17);
const THRESHOLD_18: u32 = threshold(18);
const THRESHOLD_19: u32 = threshold(19);
const THRESHOLD_20: u32 = threshold(20);
const THRESHOLD_21: u32 = threshold(21);
const THRESHOLD_22: u32 = threshold(22);
const THRESHOLD_23: u32 = threshold(23);
const THRESHOLD_24: u32 = threshold(24);
const THRESHOLD_25: u32 = threshold(25);

#[derive(Clone, Debug)]
pub struct RangeResult {
    pub best_score: u8,
    pub best_arr: [u8; N],
    pub best_index: u64,
}

#[inline(always)]
const fn threshold(max: u32) -> u32 {
    ((1u64 << 32) % (max as u64)) as u32
}

#[inline(always)]
pub fn run_range(seed: u64, lo: u64, hi: u64) -> RangeResult {
    if lo >= hi {
        return RangeResult {
            best_score: 0,
            best_arr: INITIAL_ARR,
            best_index: lo,
        };
    }

    let mut best_score = 0u8;
    let mut best_index = lo;
    let mut seed_cursor = SeedCursor::new(seed, lo);

    for it in lo..hi {
        let mut state = seed_cursor.current_state();
        let correct = score_candidate(&mut state, best_score);
        if correct > best_score {
            best_score = correct;
            best_index = it;
        }
        if it + 1 != hi {
            seed_cursor.advance();
        }
    }

    let best_arr = materialize_arr(seed, best_index);

    RangeResult {
        best_score,
        best_arr,
        best_index,
    }
}

#[inline(always)]
fn splitmix64_step(z: &mut u64) -> u64 {
    *z = z.wrapping_add(SEED_STRIDE);
    let mut value = *z;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[inline(always)]
fn xseed(seed64: u64) -> [u32; 4] {
    let mut z = seed64;
    let a = splitmix64_step(&mut z);
    let b = splitmix64_step(&mut z);
    let mut s = state_from_pair(a, b);
    if s == [0, 0, 0, 0] {
        s[0] = 1;
    }
    s
}

#[inline(always)]
fn state_from_pair(a: u64, b: u64) -> [u32; 4] {
    [
        (a & 0xffff_ffff) as u32,
        ((a >> 32) & 0xffff_ffff) as u32,
        (b & 0xffff_ffff) as u32,
        ((b >> 32) & 0xffff_ffff) as u32,
    ]
}

#[inline(always)]
fn xnext(s: &mut [u32; 4]) -> u32 {
    let res = s[0]
        .wrapping_add(s[3])
        .rotate_left(7)
        .wrapping_add(s[0]);
    let t = s[1] << 9;
    s[2] ^= s[0];
    s[3] ^= s[1];
    s[1] ^= s[2];
    s[0] ^= s[3];
    s[2] ^= t;
    s[3] = s[3].rotate_left(11);
    res
}

#[inline(always)]
fn xint(s: &mut [u32; 4], max: u32, threshold: u32) -> usize {
    loop {
        let value = xnext(s);
        if value >= threshold {
            return (value % max) as usize;
        }
    }
}

#[inline(always)]
fn swap_at(arr: &mut [u8; N], left: usize, right: usize) {
    unsafe {
        let ptr = arr.as_mut_ptr();
        std::ptr::swap(ptr.add(left), ptr.add(right));
    }
}

#[inline(always)]
fn shuffle_arr(arr: &mut [u8; N], state: &mut [u32; 4]) {
    swap_at(arr, 24, xint(state, 25, THRESHOLD_25));
    swap_at(arr, 23, xint(state, 24, THRESHOLD_24));
    swap_at(arr, 22, xint(state, 23, THRESHOLD_23));
    swap_at(arr, 21, xint(state, 22, THRESHOLD_22));
    swap_at(arr, 20, xint(state, 21, THRESHOLD_21));
    swap_at(arr, 19, xint(state, 20, THRESHOLD_20));
    swap_at(arr, 18, xint(state, 19, THRESHOLD_19));
    swap_at(arr, 17, xint(state, 18, THRESHOLD_18));
    swap_at(arr, 16, xint(state, 17, THRESHOLD_17));
    swap_at(arr, 15, xint(state, 16, THRESHOLD_16));
    swap_at(arr, 14, xint(state, 15, THRESHOLD_15));
    swap_at(arr, 13, xint(state, 14, THRESHOLD_14));
    swap_at(arr, 12, xint(state, 13, THRESHOLD_13));
    swap_at(arr, 11, xint(state, 12, THRESHOLD_12));
    swap_at(arr, 10, xint(state, 11, THRESHOLD_11));
    swap_at(arr, 9, xint(state, 10, THRESHOLD_10));
    swap_at(arr, 8, xint(state, 9, THRESHOLD_9));
    swap_at(arr, 7, xint(state, 8, THRESHOLD_8));
    swap_at(arr, 6, xint(state, 7, THRESHOLD_7));
    swap_at(arr, 5, xint(state, 6, THRESHOLD_6));
    swap_at(arr, 4, xint(state, 5, THRESHOLD_5));
    swap_at(arr, 3, xint(state, 4, THRESHOLD_4));
    swap_at(arr, 2, xint(state, 3, THRESHOLD_3));
    swap_at(arr, 1, xint(state, 2, THRESHOLD_2));
}

#[cfg(test)]
#[inline(always)]
fn count_fixed_points(arr: &[u8; N]) -> u8 {
    (arr[0] == 1) as u8
        + (arr[1] == 2) as u8
        + (arr[2] == 3) as u8
        + (arr[3] == 4) as u8
        + (arr[4] == 5) as u8
        + (arr[5] == 6) as u8
        + (arr[6] == 7) as u8
        + (arr[7] == 8) as u8
        + (arr[8] == 9) as u8
        + (arr[9] == 10) as u8
        + (arr[10] == 11) as u8
        + (arr[11] == 12) as u8
        + (arr[12] == 13) as u8
        + (arr[13] == 14) as u8
        + (arr[14] == 15) as u8
        + (arr[15] == 16) as u8
        + (arr[16] == 17) as u8
        + (arr[17] == 18) as u8
        + (arr[18] == 19) as u8
        + (arr[19] == 20) as u8
        + (arr[20] == 21) as u8
        + (arr[21] == 22) as u8
        + (arr[22] == 23) as u8
        + (arr[23] == 24) as u8
        + (arr[24] == 25) as u8
}

#[derive(Clone, Copy)]
struct SeedCursor {
    z: u64,
    a: u64,
    b: u64,
}

impl SeedCursor {
    #[inline(always)]
    fn new(seed: u64, index: u64) -> Self {
        let mut z = seed.wrapping_add(index.wrapping_mul(SEED_STRIDE));
        let a = splitmix64_step(&mut z);
        let b = splitmix64_step(&mut z);
        Self { z, a, b }
    }

    #[inline(always)]
    fn current_state(&self) -> [u32; 4] {
        state_from_pair(self.a, self.b)
    }

    #[inline(always)]
    fn advance(&mut self) {
        self.a = self.b;
        self.b = splitmix64_step(&mut self.z);
    }
}

#[inline(always)]
fn materialize_arr(seed: u64, index: u64) -> [u8; N] {
    let mut state = xseed(seed.wrapping_add(index.wrapping_mul(SEED_STRIDE)));
    let mut arr = INITIAL_ARR;
    shuffle_arr(&mut arr, &mut state);
    arr
}

macro_rules! score_step {
    ($state:expr, $foreign_hits:expr, $fixed:expr, $floor:expr, $idx:expr, $max:expr, $threshold:expr) => {{
        let draw = xint($state, $max, $threshold);
        let idx_bit = 1u32 << $idx;
        if draw == $idx {
            $fixed += (($foreign_hits & idx_bit) == 0) as u8;
        } else {
            $foreign_hits |= 1u32 << draw;
        }

        let remaining = ((!$foreign_hits) & ((1u32 << $idx) - 1) & LOW_BIT_MASK).count_ones() as u8;
        if $fixed + remaining <= $floor {
            return $floor;
        }
    }};
}

#[inline(always)]
fn score_candidate(state: &mut [u32; 4], floor: u8) -> u8 {
    let mut foreign_hits = 0u32;
    let mut fixed = 0u8;

    score_step!(state, foreign_hits, fixed, floor, 24, 25, THRESHOLD_25);
    score_step!(state, foreign_hits, fixed, floor, 23, 24, THRESHOLD_24);
    score_step!(state, foreign_hits, fixed, floor, 22, 23, THRESHOLD_23);
    score_step!(state, foreign_hits, fixed, floor, 21, 22, THRESHOLD_22);
    score_step!(state, foreign_hits, fixed, floor, 20, 21, THRESHOLD_21);
    score_step!(state, foreign_hits, fixed, floor, 19, 20, THRESHOLD_20);
    score_step!(state, foreign_hits, fixed, floor, 18, 19, THRESHOLD_19);
    score_step!(state, foreign_hits, fixed, floor, 17, 18, THRESHOLD_18);
    score_step!(state, foreign_hits, fixed, floor, 16, 17, THRESHOLD_17);
    score_step!(state, foreign_hits, fixed, floor, 15, 16, THRESHOLD_16);
    score_step!(state, foreign_hits, fixed, floor, 14, 15, THRESHOLD_15);
    score_step!(state, foreign_hits, fixed, floor, 13, 14, THRESHOLD_14);
    score_step!(state, foreign_hits, fixed, floor, 12, 13, THRESHOLD_13);
    score_step!(state, foreign_hits, fixed, floor, 11, 12, THRESHOLD_12);
    score_step!(state, foreign_hits, fixed, floor, 10, 11, THRESHOLD_11);
    score_step!(state, foreign_hits, fixed, floor, 9, 10, THRESHOLD_10);
    score_step!(state, foreign_hits, fixed, floor, 8, 9, THRESHOLD_9);
    score_step!(state, foreign_hits, fixed, floor, 7, 8, THRESHOLD_8);
    score_step!(state, foreign_hits, fixed, floor, 6, 7, THRESHOLD_7);
    score_step!(state, foreign_hits, fixed, floor, 5, 6, THRESHOLD_6);
    score_step!(state, foreign_hits, fixed, floor, 4, 5, THRESHOLD_5);
    score_step!(state, foreign_hits, fixed, floor, 3, 4, THRESHOLD_4);
    score_step!(state, foreign_hits, fixed, floor, 2, 3, THRESHOLD_3);
    score_step!(state, foreign_hits, fixed, floor, 1, 2, THRESHOLD_2);

    fixed + ((foreign_hits & 1) == 0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_kernel_matches_known_output() {
        let result = run_range(1_234_567_890_123_456_789, 0, 1_000);
        assert_eq!(result.best_score, 6);
        assert_eq!(result.best_index, 866);
        assert_eq!(
            result.best_arr,
            [
                1, 22, 4, 15, 5, 17, 13, 12, 11, 25, 10, 21, 9, 7, 18, 14, 8, 3, 19, 20, 16,
                2, 23, 24, 6,
            ]
        );
    }

    #[test]
    fn score_screen_matches_materialized_counts() {
        let seed = 1_234_567_890_123_456_789u64;
        for index in 0..10_000u64 {
            let mut state = SeedCursor::new(seed, index).current_state();
            let score = score_candidate(&mut state, 0);
            let arr = materialize_arr(seed, index);
            assert_eq!(score, count_fixed_points(&arr), "index={index}");
        }
    }
}