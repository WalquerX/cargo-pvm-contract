//! Const-evaluable Keccak-256 implementation for compile-time selector computation.
//! This is a minimal implementation of the FIPS 202 / Keccak sponge construction.

const RATE: usize = 136; // rate for Keccak-256 (1600 - 2*256) / 8
const STATE_SIZE: usize = 25; // 5x5 u64 words

const RC: [u64; 24] = [
    0x0000000000000001,
    0x0000000000008082,
    0x800000000000808A,
    0x8000000080008000,
    0x000000000000808B,
    0x0000000080000001,
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008A,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000A,
    0x000000008000808B,
    0x800000000000008B,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
    0x000000000000800A,
    0x800000008000000A,
    0x8000000080008081,
    0x8000000000008080,
    0x0000000080000001,
    0x8000000080008008,
];

const ROT: [u32; 24] = [
    1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
];

const PI: [usize; 24] = [
    10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
];

const fn keccak_f(state: &mut [u64; STATE_SIZE]) {
    let mut round = 0;
    while round < 24 {
        // theta
        let mut c = [0u64; 5];
        let mut i = 0;
        while i < 5 {
            c[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
            i += 1;
        }
        let mut i = 0;
        while i < 5 {
            let d = c[(i + 4) % 5] ^ c[(i + 1) % 5].rotate_left(1);
            let mut j = 0;
            while j < 25 {
                state[j + i] ^= d;
                j += 5;
            }
            i += 1;
        }

        // rho and pi
        let mut current = state[1];
        let mut i = 0;
        while i < 24 {
            let j = PI[i];
            let temp = state[j];
            state[j] = current.rotate_left(ROT[i]);
            current = temp;
            i += 1;
        }

        // chi
        let mut y = 0;
        while y < 25 {
            let mut t = [0u64; 5];
            let mut x = 0;
            while x < 5 {
                t[x] = state[y + x];
                x += 1;
            }
            let mut x = 0;
            while x < 5 {
                state[y + x] = t[x] ^ ((!t[(x + 1) % 5]) & t[(x + 2) % 5]);
                x += 1;
            }
            y += 5;
        }

        // iota
        state[0] ^= RC[round];
        round += 1;
    }
}

pub const fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut state = [0u64; STATE_SIZE];
    let mut offset = 0;
    let len = data.len();

    // absorb full blocks
    while offset + RATE <= len {
        let mut i = 0;
        while i < RATE / 8 {
            let b = [
                data[offset + i * 8],
                data[offset + i * 8 + 1],
                data[offset + i * 8 + 2],
                data[offset + i * 8 + 3],
                data[offset + i * 8 + 4],
                data[offset + i * 8 + 5],
                data[offset + i * 8 + 6],
                data[offset + i * 8 + 7],
            ];
            state[i] ^= u64::from_le_bytes(b);
            i += 1;
        }
        keccak_f(&mut state);
        offset += RATE;
    }

    // absorb last block with padding
    let mut last_block = [0u8; RATE];
    let remaining = len - offset;
    let mut i = 0;
    while i < remaining {
        last_block[i] = data[offset + i];
        i += 1;
    }
    last_block[remaining] = 0x01; // Keccak padding (NOT SHA-3 which uses 0x06)
    last_block[RATE - 1] |= 0x80;

    let mut i = 0;
    while i < RATE / 8 {
        let b = [
            last_block[i * 8],
            last_block[i * 8 + 1],
            last_block[i * 8 + 2],
            last_block[i * 8 + 3],
            last_block[i * 8 + 4],
            last_block[i * 8 + 5],
            last_block[i * 8 + 6],
            last_block[i * 8 + 7],
        ];
        state[i] ^= u64::from_le_bytes(b);
        i += 1;
    }
    keccak_f(&mut state);

    // squeeze 32 bytes
    let mut output = [0u8; 32];
    let mut i = 0;
    while i < 4 {
        let bytes = state[i].to_le_bytes();
        output[i * 8] = bytes[0];
        output[i * 8 + 1] = bytes[1];
        output[i * 8 + 2] = bytes[2];
        output[i * 8 + 3] = bytes[3];
        output[i * 8 + 4] = bytes[4];
        output[i * 8 + 5] = bytes[5];
        output[i * 8 + 6] = bytes[6];
        output[i * 8 + 7] = bytes[7];
        i += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak256_empty() {
        let hash = keccak256(b"");
        let expected: [u8; 32] = [
            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
            0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
            0x5d, 0x85, 0xa4, 0x70,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn keccak256_transfer_selector() {
        let hash = keccak256(b"transfer(address,uint256)");
        assert_eq!(
            [hash[0], hash[1], hash[2], hash[3]],
            [0xa9, 0x05, 0x9c, 0xbb]
        );
    }

    #[test]
    fn keccak256_balance_of_selector() {
        let hash = keccak256(b"balanceOf(address)");
        assert_eq!(
            [hash[0], hash[1], hash[2], hash[3]],
            [0x70, 0xa0, 0x82, 0x31]
        );
    }

    #[test]
    fn const_selector_matches_tiny_keccak() {
        // Verify our const implementation matches tiny-keccak's results
        let sel = crate::const_selector("transfer(address,uint256)");
        assert_eq!(sel, [0xa9, 0x05, 0x9c, 0xbb]);

        let sel = crate::const_selector("totalSupply()");
        assert_eq!(sel, [0x18, 0x16, 0x0d, 0xdd]);

        let sel = crate::const_selector("mint(address,uint256)");
        assert_eq!(sel, [0x40, 0xc1, 0x0f, 0x19]);
    }
}
