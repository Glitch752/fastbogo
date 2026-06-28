#[derive(Debug, Clone, Copy)]
pub struct MagicParams {
    pub m: u32,
    pub sh1: u32,
    pub sh2: u32,
}

pub const fn calculate_magic(d: u32) -> MagicParams {
    match d {
        0 => panic!("Divisor cannot be 0"),
        1 => MagicParams { m: 1, sh1: 0, sh2: 0 },
        2 => MagicParams { m: 1, sh1: 1, sh2: 0 },
        _ => {
            // L = ceil(log2(d))
            // Equivalent to: 32 - (d-1).leading_zeros()
            let l = 32 - (d - 1).leading_zeros();
            let l2: u64 = if l < 32 { 1 << l } else { 0 };
            
            // Magic multiplier calculation
            let m = 1 + (((l2 - d as u64) << 32) / d as u64) as u32;
            
            MagicParams {
                m,
                sh1: 1,
                sh2: l - 1,
            }
        }
    }
}

pub const fn magic_m<const D: u32>() -> i32 {
    calculate_magic(D).m as i32
}

pub const fn magic_sh1<const D: u32>() -> i32 {
    calculate_magic(D).sh1 as i32
}

pub const fn magic_sh2<const D: u32>() -> i32 {
    calculate_magic(D).sh2 as i32
}