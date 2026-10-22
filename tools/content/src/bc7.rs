//! A BC7 decoder, for the value budget's test: the baked base maps are block
//! compressed, and the budget is a statement about the colour a texel shows,
//! so the test reads the texels the way the GPU would.
//!
//! The partition and anchor tables are the BC7 format's (the same numbers the
//! encoder in `ctt-bc7enc-rdo` carries); nothing else here is shared with it.

#![allow(
    clippy::cast_possible_truncation,
    reason = "bit fields of at most eight bits and texel bytes the format bounds"
)]

const PARTITION2: [u8; 1024] = [
    0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
    0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1,
    0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1,
    0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0,
    0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1,
    0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0,
    0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0,
    0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0,
    0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1,
    0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0,
    0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0,
    0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1,
    0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0,
    0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0,
    0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1,
    0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0,
    0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0,
    0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1,
    0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0,
    0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1,
    0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1,
    0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0,
    0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1,
];

const PARTITION3: [u8; 1024] = [
    0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2, 0, 0, 0, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1,
    0, 0, 0, 0, 2, 0, 0, 1, 2, 2, 1, 1, 2, 2, 1, 1, 0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 1, 0, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 2, 2,
    0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2,
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2,
    0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2,
    0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2, 0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 2, 2, 2, 0,
    0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2, 0, 1, 1, 1, 0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0,
    0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1,
    0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2, 0, 2, 2, 2, 0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 2, 1, 2, 2, 2, 1,
    0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 0, 0, 1, 1, 0, 0, 2, 2, 1, 0, 2, 2, 1, 0,
    0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 1, 2, 1, 1, 2, 2, 2, 2, 2, 2,
    0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1,
    0, 0, 2, 2, 1, 1, 0, 2, 1, 1, 0, 2, 0, 0, 2, 2, 0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 0, 2, 2, 2, 2, 2,
    0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0, 2, 0, 0, 0, 2, 2, 1, 1, 2, 2, 2, 1,
    0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 2, 0, 0, 1, 1,
    0, 0, 1, 1, 0, 0, 1, 2, 0, 0, 2, 2, 0, 2, 2, 2, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0,
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0,
    0, 1, 2, 0, 2, 0, 1, 2, 1, 2, 0, 1, 0, 1, 2, 0, 0, 0, 1, 1, 2, 2, 0, 0, 1, 1, 2, 2, 0, 0, 1, 1,
    0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2,
    0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1, 0, 0, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 1, 1, 2, 2,
    0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 1, 1, 0, 2, 2, 0, 1, 2, 2, 1, 0, 2, 2, 0, 1, 2, 2, 1,
    0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 0, 1, 0, 1, 0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2, 0, 2, 2, 2, 0, 1, 1, 1, 0, 2, 2, 2, 0, 1, 1, 1,
    0, 0, 0, 2, 1, 1, 1, 2, 0, 0, 0, 2, 1, 1, 1, 2, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2,
    0, 2, 2, 2, 0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2, 0, 0, 0, 2, 1, 1, 1, 2, 1, 1, 1, 2, 0, 0, 0, 2,
    0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2,
    0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2,
    0, 0, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2,
    0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 2, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 1, 2, 2, 2,
    0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 0, 1, 1, 1, 2, 0, 1, 1, 2, 2, 0, 1, 2, 2, 2, 0,
];

const ANCHOR2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8,
    2, 2, 8, 8, 2, 2, 15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2,
    2, 15, 15, 15, 15, 15, 2, 2, 15,
];

const ANCHOR3_1: [u8; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5,
    15, 15, 8, 15, 3, 5, 6, 10, 8, 15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5,
    10, 8, 13, 15, 12, 3, 3,
];

const ANCHOR3_2: [u8; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6,
    10, 15, 15, 10, 8, 15, 3, 15, 10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];

const SUBSETS: [usize; 8] = [3, 2, 3, 2, 1, 1, 1, 2];
const PARTITION_BITS: [u32; 8] = [4, 6, 6, 6, 0, 0, 0, 6];
const ROTATION_BITS: [u32; 8] = [0, 0, 0, 0, 2, 2, 0, 0];
const SELECTION_BITS: [u32; 8] = [0, 0, 0, 0, 1, 0, 0, 0];
const COLOUR_BITS: [u32; 8] = [4, 6, 5, 7, 5, 7, 7, 5];
const ALPHA_BITS: [u32; 8] = [0, 0, 0, 0, 6, 8, 7, 5];
const UNIQUE_P: [bool; 8] = [true, false, false, true, false, false, true, true];
const SHARED_P: [bool; 8] = [false, true, false, false, false, false, false, false];
const INDEX_BITS: [u32; 8] = [3, 3, 2, 2, 2, 2, 4, 2];
const ALPHA_INDEX_BITS: [u32; 8] = [0, 0, 0, 0, 3, 2, 0, 0];

const WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
const WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weight(bits: u32, index: u32) -> u32 {
    match bits {
        2 => WEIGHTS2[index as usize],
        3 => WEIGHTS3[index as usize],
        _ => WEIGHTS4[index as usize],
    }
}

struct Bits {
    word: u128,
    at: u32,
}

impl Bits {
    fn take(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = (self.word >> self.at) & ((1u128 << n) - 1);
        self.at += n;
        // At most 8 bits are asked for at a time.
        v as u32
    }
}

fn unquantise(value: u32, bits: u32) -> u32 {
    let v = value << (8 - bits);
    v | (v >> bits)
}

/// One 16-byte block as 16 RGBA texels, row by row.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one pass down the format's bit layout reads best unbroken"
)]
pub fn decode_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let mut bits = Bits {
        word: u128::from_le_bytes(*block),
        at: 0,
    };
    let mode = (block[0] | 0x80).trailing_zeros() as usize;
    if mode >= 8 {
        return [[0, 0, 0, 255]; 16];
    }
    bits.at = mode as u32 + 1;
    let partition = bits.take(PARTITION_BITS[mode]) as usize;
    let rotation = bits.take(ROTATION_BITS[mode]);
    let selection = bits.take(SELECTION_BITS[mode]);
    let subsets = SUBSETS[mode];
    let ends = subsets * 2;
    let mut colour = [[0u32; 3]; 6];
    for channel in 0..3 {
        for end in colour.iter_mut().take(ends) {
            end[channel] = bits.take(COLOUR_BITS[mode]);
        }
    }
    let mut alpha = [255u32; 6];
    if ALPHA_BITS[mode] > 0 {
        for a in alpha.iter_mut().take(ends) {
            *a = bits.take(ALPHA_BITS[mode]);
        }
    }
    let mut p = [0u32; 6];
    if UNIQUE_P[mode] {
        for v in p.iter_mut().take(ends) {
            *v = bits.take(1);
        }
    } else if SHARED_P[mode] {
        for s in 0..subsets {
            let v = bits.take(1);
            p[s * 2] = v;
            p[s * 2 + 1] = v;
        }
    }
    let has_p = UNIQUE_P[mode] || SHARED_P[mode];
    let cprec = COLOUR_BITS[mode] + u32::from(has_p);
    let aprec = ALPHA_BITS[mode] + u32::from(has_p && ALPHA_BITS[mode] > 0);
    let mut rgba = [[0u32; 4]; 6];
    for e in 0..ends {
        for c in 0..3 {
            let v = if has_p {
                (colour[e][c] << 1) | p[e]
            } else {
                colour[e][c]
            };
            rgba[e][c] = unquantise(v, cprec);
        }
        rgba[e][3] = if ALPHA_BITS[mode] == 0 {
            255
        } else {
            let v = if has_p {
                (alpha[e] << 1) | p[e]
            } else {
                alpha[e]
            };
            unquantise(v, aprec)
        };
    }
    let subset_of = |i: usize| -> usize {
        match subsets {
            1 => 0,
            2 => usize::from(PARTITION2[partition * 16 + i]),
            _ => usize::from(PARTITION3[partition * 16 + i]),
        }
    };
    let is_anchor = |i: usize| -> bool {
        match subset_of(i) {
            0 => i == 0,
            1 => {
                i == if subsets == 2 {
                    usize::from(ANCHOR2[partition])
                } else {
                    usize::from(ANCHOR3_1[partition])
                }
            }
            _ => i == usize::from(ANCHOR3_2[partition]),
        }
    };
    let cbits = INDEX_BITS[mode];
    let mut first = [0u32; 16];
    // Mode 4 stores its 2-bit set first whichever set the selection bit
    // gives to the colour.
    let first_bits = if mode == 4 { 2 } else { cbits };
    for (i, slot) in first.iter_mut().enumerate() {
        let n = if is_anchor(i) {
            first_bits - 1
        } else {
            first_bits
        };
        *slot = bits.take(n);
    }
    let mut second = first;
    let mut second_bits = first_bits;
    if ALPHA_INDEX_BITS[mode] > 0 {
        second_bits = ALPHA_INDEX_BITS[mode];
        for (i, slot) in second.iter_mut().enumerate() {
            let n = if i == 0 { second_bits - 1 } else { second_bits };
            *slot = bits.take(n);
        }
    }
    let mut out = [[0u8; 4]; 16];
    for i in 0..16 {
        let s = subset_of(i);
        let (e0, e1) = (rgba[s * 2], rgba[s * 2 + 1]);
        let (ci, cb, ai, ab) = if ALPHA_INDEX_BITS[mode] == 0 {
            (first[i], first_bits, first[i], first_bits)
        } else if selection == 1 {
            (second[i], second_bits, first[i], first_bits)
        } else {
            (first[i], first_bits, second[i], second_bits)
        };
        let (cw, aw) = (weight(cb, ci), weight(ab, ai));
        let mut px = [0u32; 4];
        for c in 0..3 {
            px[c] = ((64 - cw) * e0[c] + cw * e1[c] + 32) >> 6;
        }
        px[3] = ((64 - aw) * e0[3] + aw * e1[3] + 32) >> 6;
        match rotation {
            1 => px.swap(0, 3),
            2 => px.swap(1, 3),
            3 => px.swap(2, 3),
            _ => {}
        }
        out[i] = [px[0] as u8, px[1] as u8, px[2] as u8, px[3] as u8];
    }
    out
}

/// A level of BC7 blocks as RGBA texels, `width` by `height` (both multiples
/// of four).
#[must_use]
pub fn decode_level(blocks: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; width * height];
    let across = width / 4;
    for (n, chunk) in blocks.as_chunks::<16>().0.iter().enumerate() {
        let texels = decode_block(chunk);
        let (bx, by) = (n % across * 4, n / across * 4);
        for (i, t) in texels.iter().enumerate() {
            out[(by + i / 4) * width + bx + i % 4] = *t;
        }
    }
    out
}
