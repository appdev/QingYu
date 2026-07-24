use crate::sha1_hex;

const WINDOW_SIZE: usize = 64;
const MIN_SIZE: usize = 512 * 1024;
const MAX_SIZE: usize = 8 * 1024 * 1024;
const SPLIT_MASK: u64 = (1 << 20) - 1;
const POLYNOMIAL: u64 = 0x3DA3358B4DC173;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChunkBoundary {
    pub offset: usize,
    pub length: usize,
    pub sha1: String,
}

pub struct RabinChunker<'a> {
    bytes: &'a [u8],
    offset: usize,
    tables: Tables,
}

#[derive(Clone)]
struct Tables {
    outgoing: [u64; 256],
    modulus: [u64; 256],
    polynomial_shift: u32,
}

impl<'a> RabinChunker<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            tables: Tables::new(),
        }
    }

    fn next_length(&self) -> usize {
        let remaining = self.bytes.len() - self.offset;
        let mut state = RabinState::new(&self.tables);
        let skipped = (MIN_SIZE - WINDOW_SIZE).min(remaining);
        state.count += skipped;

        for (relative, byte) in self.bytes[self.offset + skipped..]
            .iter()
            .copied()
            .enumerate()
        {
            state.slide(byte, &self.tables);
            if state.count >= MIN_SIZE
                && ((state.digest & SPLIT_MASK) == 0 || state.count >= MAX_SIZE)
            {
                return skipped + relative + 1;
            }
        }

        remaining
    }
}

impl Iterator for RabinChunker<'_> {
    type Item = ChunkBoundary;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.bytes.len() {
            return None;
        }

        let length = self.next_length();
        let offset = self.offset;
        let chunk = &self.bytes[offset..offset + length];
        self.offset += length;
        Some(ChunkBoundary {
            offset,
            length,
            sha1: sha1_hex(chunk),
        })
    }
}

struct RabinState {
    window: [u8; WINDOW_SIZE],
    window_position: usize,
    digest: u64,
    count: usize,
}

impl RabinState {
    fn new(tables: &Tables) -> Self {
        let mut state = Self {
            window: [0; WINDOW_SIZE],
            window_position: 0,
            digest: 0,
            count: 0,
        };
        state.slide(1, tables);
        state.count = 0;
        state
    }

    fn slide(&mut self, byte: u8, tables: &Tables) {
        let position = self.window_position % WINDOW_SIZE;
        let outgoing = self.window[position];
        self.window[position] = byte;
        self.digest ^= tables.outgoing[usize::from(outgoing)];
        self.window_position = (self.window_position + 1) % WINDOW_SIZE;
        self.digest = update_digest(self.digest, tables.polynomial_shift, &tables.modulus, byte);
        self.count += 1;
    }
}

impl Tables {
    fn new() -> Self {
        let polynomial_shift = polynomial_degree(POLYNOMIAL) - 8;
        let mut outgoing = [0; 256];
        let mut modulus = [0; 256];

        for byte in 0_u16..=255 {
            let mut hash = append_byte(0, byte as u8);
            for _ in 0..WINDOW_SIZE - 1 {
                hash = append_byte(hash, 0);
            }
            outgoing[usize::from(byte)] = hash;
        }

        let degree = polynomial_degree(POLYNOMIAL);
        for byte in 0_u16..=255 {
            let shifted = u64::from(byte) << degree;
            modulus[usize::from(byte)] = polynomial_mod(shifted) | shifted;
        }

        Self {
            outgoing,
            modulus,
            polynomial_shift,
        }
    }
}

fn update_digest(digest: u64, shift: u32, modulus: &[u64; 256], byte: u8) -> u64 {
    let index = (digest >> shift) as usize;
    ((digest << 8) | u64::from(byte)) ^ modulus[index]
}

fn append_byte(hash: u64, byte: u8) -> u64 {
    polynomial_mod((hash << 8) | u64::from(byte))
}

fn polynomial_mod(mut dividend: u64) -> u64 {
    let divisor_degree = polynomial_degree(POLYNOMIAL);
    while dividend != 0 {
        let dividend_degree = polynomial_degree(dividend);
        if dividend_degree < divisor_degree {
            return dividend;
        }
        dividend ^= POLYNOMIAL << (dividend_degree - divisor_degree);
    }
    0
}

fn polynomial_degree(value: u64) -> u32 {
    u64::BITS - value.leading_zeros() - 1
}

#[cfg(test)]
mod tests {
    use super::{ChunkBoundary, RabinChunker};

    fn golden_stream() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(20 * 1024 * 1024);
        let mut x = 0x4d595df4d0f33173_u64;
        for _ in 0..bytes.capacity() {
            x ^= x.wrapping_shl(13);
            x ^= x.wrapping_shr(7);
            x ^= x.wrapping_shl(17);
            bytes.push(x as u8);
        }
        bytes
    }

    #[test]
    fn boundaries_match_the_pinned_restic_go_oracle() {
        let expected_chunks: Vec<ChunkBoundary> = serde_json::from_str(include_str!(
            "../tests/fixtures/golden/chunk-boundaries.json"
        ))
        .unwrap();
        let bytes = golden_stream();

        assert_eq!(
            RabinChunker::new(&bytes).collect::<Vec<_>>(),
            expected_chunks
        );
    }
}
