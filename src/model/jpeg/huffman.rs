//! JPEG's Huffman layer (ITU T.81 Annex C and F): tables, the bit reader and writer with their
//! 0xFF stuffing, and optimal tables built from symbol counts, as libjpeg's `-optimize` does.

use std::io;

use super::markers::bad;

/// A table as DHT carries it: how many codes there are of each length 1..=16, then the symbols
/// in code order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub counts: [u8; 16],
    pub symbols: Vec<u8>,
}

impl Table {
    /// The (code, length) of every symbol, in table order.
    fn codes(&self) -> io::Result<Vec<(u16, u8)>> {
        let mut out = Vec::with_capacity(self.symbols.len());
        let mut code: u32 = 0;
        for (i, &n) in self.counts.iter().enumerate() {
            let len = i as u8 + 1;
            for _ in 0..n {
                if code >= 1 << len {
                    return Err(bad("Huffman table"));
                }
                out.push((code as u16, len));
                code += 1;
            }
            code <<= 1;
        }
        if out.len() != self.symbols.len() {
            return Err(bad("Huffman table"));
        }
        Ok(out)
    }
}

const LOOKAHEAD: u32 = 9;

pub struct Decoder {
    /// Indexed by the next 9 bits: (length, symbol), length 0 when the code is longer.
    fast: Vec<(u8, u8)>,
    /// Codes sorted by length then value, for the slow path.
    codes: Vec<(u16, u8, u8)>,
}

impl Decoder {
    pub fn new(table: &Table) -> io::Result<Self> {
        let codes = table.codes()?;
        let mut fast = vec![(0u8, 0u8); 1 << LOOKAHEAD];
        let mut slow = Vec::with_capacity(codes.len());
        for (&(code, len), &symbol) in codes.iter().zip(&table.symbols) {
            if u32::from(len) <= LOOKAHEAD {
                let shift = LOOKAHEAD - u32::from(len);
                let base = (code as usize) << shift;
                for e in &mut fast[base..base + (1 << shift)] {
                    *e = (len, symbol);
                }
            }
            slow.push((code, len, symbol));
        }
        Ok(Self { fast, codes: slow })
    }

    pub fn decode(&self, r: &mut BitReader) -> io::Result<u8> {
        r.fill();
        let (len, symbol) = self.fast[r.peek(LOOKAHEAD) as usize];
        if len > 0 {
            r.consume(u32::from(len));
            return Ok(symbol);
        }
        let bits = r.peek(16) as u16;
        for &(code, len, symbol) in &self.codes {
            if len as u32 > LOOKAHEAD && bits >> (16 - len) == code {
                r.consume(u32::from(len));
                return Ok(symbol);
            }
        }
        Err(bad("bad Huffman code"))
    }
}

/// Every symbol's code and length, ready to write.
pub struct Encoder {
    code: [u16; 256],
    len: [u8; 256],
}

impl Encoder {
    pub fn new(table: &Table) -> io::Result<Self> {
        let mut code = [0u16; 256];
        let mut len = [0u8; 256];
        for (&(c, l), &s) in table.codes()?.iter().zip(&table.symbols) {
            code[s as usize] = c;
            len[s as usize] = l;
        }
        Ok(Self { code, len })
    }

    pub fn put(&self, w: &mut BitWriter, symbol: u8) {
        debug_assert!(self.len[symbol as usize] > 0, "symbol {symbol} has no code");
        w.put(
            u32::from(self.code[symbol as usize]),
            u32::from(self.len[symbol as usize]),
        );
    }
}

/// The optimal length-limited table for these symbol counts (libjpeg's `jpeg_gen_optimal_table`).
pub fn optimal(freq: &[u32; 256]) -> Table {
    let mut freq: Vec<u64> = freq.iter().map(|&f| u64::from(f)).collect();
    // A reserved symbol keeps any real code from being all ones.
    freq.push(1);
    let mut size = [0usize; 257];
    let mut others = [usize::MAX; 257];
    loop {
        let smallest = |skip: usize, freq: &[u64]| {
            let mut best = None;
            let mut v = u64::MAX;
            for (i, &f) in freq.iter().enumerate() {
                if f > 0 && f <= v && i != skip {
                    v = f;
                    best = Some(i);
                }
            }
            best
        };
        let Some(mut c1) = smallest(usize::MAX, &freq) else {
            break;
        };
        let Some(mut c2) = smallest(c1, &freq) else {
            break;
        };
        freq[c1] += freq[c2];
        freq[c2] = 0;
        size[c1] += 1;
        while others[c1] != usize::MAX {
            c1 = others[c1];
            size[c1] += 1;
        }
        others[c1] = c2;
        size[c2] += 1;
        while others[c2] != usize::MAX {
            c2 = others[c2];
            size[c2] += 1;
        }
    }
    let mut bits = [0u32; 33];
    for &s in &size {
        if s > 0 {
            bits[s.min(32)] += 1;
        }
    }
    for i in (17..=32).rev() {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
    }
    let mut longest = 16;
    while bits[longest] == 0 {
        longest -= 1;
    }
    bits[longest] -= 1;
    let mut symbols = Vec::new();
    for len in 1..=32 {
        for (s, &sz) in size.iter().enumerate().take(256) {
            if sz == len {
                symbols.push(s as u8);
            }
        }
    }
    let mut counts = [0u8; 16];
    for (i, c) in counts.iter_mut().enumerate() {
        *c = bits[i + 1] as u8;
    }
    Table { counts, symbols }
}

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    bits: u32,
    /// Set once a marker is reached; from then on the reader yields zeros.
    at_marker: bool,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            acc: 0,
            bits: 0,
            at_marker: false,
        }
    }

    fn fill(&mut self) {
        while self.bits <= 56 {
            let mut byte = 0u8;
            if !self.at_marker {
                match self.data.get(self.pos) {
                    None => self.at_marker = true,
                    Some(&0xFF) => match self.data.get(self.pos + 1) {
                        Some(0) => {
                            byte = 0xFF;
                            self.pos += 2;
                        }
                        _ => self.at_marker = true,
                    },
                    Some(&b) => {
                        byte = b;
                        self.pos += 1;
                    }
                }
            }
            self.acc |= u64::from(byte) << (56 - self.bits);
            self.bits += 8;
        }
    }

    fn peek(&self, n: u32) -> u32 {
        (self.acc >> (64 - n)) as u32
    }

    fn consume(&mut self, n: u32) {
        self.acc <<= n;
        self.bits -= n;
    }

    pub fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.fill();
        let v = self.peek(n);
        self.consume(n);
        v
    }

    /// `n` bits as a signed coefficient (T.81 F.2.2.1 EXTEND).
    pub fn signed(&mut self, n: u32) -> i32 {
        let v = self.bits(n) as i32;
        if n == 0 {
            0
        } else if v < 1 << (n - 1) {
            v - (1 << n) + 1
        } else {
            v
        }
    }

    /// Skips to just past the next restart marker and starts afresh.
    pub fn restart(&mut self) {
        self.acc = 0;
        self.bits = 0;
        self.at_marker = false;
        while self.pos + 1 < self.data.len() {
            let (a, b) = (self.data[self.pos], self.data[self.pos + 1]);
            self.pos += 1;
            if a == 0xFF && (0xD0..=0xD7).contains(&b) {
                self.pos += 1;
                return;
            }
        }
    }
}

#[derive(Default)]
pub struct BitWriter {
    pub out: Vec<u8>,
    acc: u64,
    bits: u32,
}

impl BitWriter {
    pub fn put(&mut self, value: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | u64::from(value & ((1 << len) - 1));
        self.bits += len;
        while self.bits >= 8 {
            self.bits -= 8;
            let byte = (self.acc >> self.bits) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0);
            }
        }
    }

    /// Pads the last byte with ones, as T.81 asks.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.put((1 << pad) - 1, pad);
        }
        self.out
    }
}

/// The magnitude category of a coefficient (T.81 F.1.2.1) and the bits that follow its code.
pub fn category(v: i32) -> (u32, u32) {
    let size = 32 - v.unsigned_abs().leading_zeros();
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (size, bits & ((1u32 << size) - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimal_tables_give_frequent_symbols_short_codes_and_stay_within_16_bits() {
        let mut freq = [0u32; 256];
        for (i, f) in freq.iter_mut().enumerate() {
            // A wildly skewed spread forces lengths past 16 before the limit pass.
            *f = if i < 40 { 1 << (i % 31) } else { 0 };
        }
        let table = optimal(&freq);
        assert_eq!(table.symbols.len(), 40);
        assert!(table.counts.iter().map(|&c| c as usize).sum::<usize>() == 40);
        let codes = table.codes().unwrap();
        assert!(codes.iter().all(|c| c.1 <= 16));
        let len_of = |s: u8| codes[table.symbols.iter().position(|&x| x == s).unwrap()].1;
        assert!(len_of(30) <= len_of(0));
        // No code is all ones.
        assert!(codes.iter().all(|&(c, l)| u32::from(c) != (1 << l) - 1));
    }

    #[test]
    fn written_symbols_read_back_through_stuffing() {
        let mut freq = [0u32; 256];
        let message: Vec<u8> = (0..2000u32).map(|i| ((i * 7919) % 23) as u8 * 11).collect();
        for &s in &message {
            freq[s as usize] += 1;
        }
        freq[0xFF] += 1;
        let table = optimal(&freq);
        let enc = Encoder::new(&table).unwrap();
        let mut w = BitWriter::default();
        for &s in &message {
            enc.put(&mut w, s);
            w.put(0xFF, 8);
            let (n, bits) = category(-(s as i32));
            w.put(bits, n);
        }
        let data = w.finish();
        let dec = Decoder::new(&table).unwrap();
        let mut r = BitReader::new(&data);
        for &s in &message {
            assert_eq!(dec.decode(&mut r).unwrap(), s);
            assert_eq!(r.bits(8), 0xFF);
            let (n, _) = category(-(s as i32));
            assert_eq!(r.signed(n), -(s as i32));
        }
    }

    #[test]
    fn categories_match_t81() {
        assert_eq!(category(0), (0, 0));
        assert_eq!(category(1), (1, 1));
        assert_eq!(category(-1), (1, 0));
        assert_eq!(category(-3), (2, 0));
        assert_eq!(category(255), (8, 255));
        assert_eq!(category(-1024), (11, 1023));
    }
}
