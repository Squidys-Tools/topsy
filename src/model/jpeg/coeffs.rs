//! A JPEG's quantised DCT coefficients, read from baseline, extended and progressive Huffman
//! files. Transforms that must not lose quality (rotation, flips, table optimisation) work here,
//! below the pixels.

use std::io;

use super::huffman::{BitReader, Decoder, Table};
use super::markers::{self, DHT, DQT, DRI, EOI, SOS, bad};

/// Zigzag position to natural (row-major) position.
pub const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

pub type Block = [i16; 64];

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub id: u8,
    pub h: u8,
    pub v: u8,
    /// Which of the four quantisation tables it uses.
    pub tq: u8,
    /// The block grid, padded out to whole MCUs.
    pub bw: usize,
    pub bh: usize,
    /// Natural order, quantised.
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Jpeg {
    pub width: usize,
    pub height: usize,
    pub components: Vec<Component>,
    /// Natural order.
    pub qtables: [[u16; 64]; 4],
    /// APPn and COM payloads, in file order, to write back.
    pub extras: Vec<(u8, Vec<u8>)>,
}

impl Jpeg {
    pub fn hmax(&self) -> usize {
        self.components
            .iter()
            .map(|c| c.h as usize)
            .max()
            .unwrap_or(1)
    }

    pub fn vmax(&self) -> usize {
        self.components
            .iter()
            .map(|c| c.v as usize)
            .max()
            .unwrap_or(1)
    }

    /// The blocks of component `c` that hold image (the rest is MCU padding).
    pub fn used_blocks(&self, c: &Component) -> (usize, usize) {
        let w = (self.width * c.h as usize).div_ceil(self.hmax());
        let h = (self.height * c.v as usize).div_ceil(self.vmax());
        (w.div_ceil(8), h.div_ceil(8))
    }

    pub fn mcus(&self) -> (usize, usize) {
        (
            self.width.div_ceil(8 * self.hmax()),
            self.height.div_ceil(8 * self.vmax()),
        )
    }

    /// Sizes the block grids for these dimensions and sampling factors.
    pub fn allocate(&mut self) {
        let (mx, my) = self.mcus();
        for c in &mut self.components {
            c.bw = mx * c.h as usize;
            c.bh = my * c.v as usize;
            c.blocks = vec![[0; 64]; c.bw * c.bh];
        }
    }
}

struct Scan {
    comps: Vec<(usize, usize, usize)>, // component index, DC table, AC table
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

/// The coefficients of a JPEG, or an error for a file this reader can't take (arithmetic
/// coding, lossless or hierarchical JPEG, 12-bit samples).
pub fn read(bytes: &[u8]) -> io::Result<Jpeg> {
    let unsupported = || {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "This kind of JPEG can't be transformed losslessly",
        )
    };
    let segments = markers::segments(bytes)?;
    let mut jpeg = Jpeg {
        width: 0,
        height: 0,
        components: Vec::new(),
        qtables: [[1; 64]; 4],
        extras: Vec::new(),
    };
    let mut dc: [Option<Decoder>; 4] = Default::default();
    let mut ac: [Option<Decoder>; 4] = Default::default();
    let mut restart = 0usize;
    let mut progressive = false;
    let mut seen_frame = false;
    for s in &segments {
        let d = s.data;
        match s.marker {
            DQT => {
                let mut p = 0;
                while p < d.len() {
                    let (precision, id) = (d[p] >> 4, (d[p] & 15) as usize);
                    p += 1;
                    if id > 3 {
                        return Err(bad("quantisation table"));
                    }
                    for &z in &ZIGZAG {
                        let v = if precision == 0 {
                            *d.get(p).ok_or_else(|| bad("quantisation table"))? as u16
                        } else {
                            let b = d.get(p..p + 2).ok_or_else(|| bad("quantisation table"))?;
                            p += 1;
                            u16::from_be_bytes([b[0], b[1]])
                        };
                        p += 1;
                        jpeg.qtables[id][z] = v.max(1);
                    }
                }
            }
            DHT => {
                let mut p = 0;
                while p + 17 <= d.len() {
                    let (class, id) = (d[p] >> 4, (d[p] & 15) as usize);
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&d[p + 1..p + 17]);
                    let n: usize = counts.iter().map(|&c| c as usize).sum();
                    let symbols = d
                        .get(p + 17..p + 17 + n)
                        .ok_or_else(|| bad("Huffman table"))?
                        .to_vec();
                    p += 17 + n;
                    if id > 3 {
                        return Err(bad("Huffman table"));
                    }
                    let decoder = Decoder::new(&Table { counts, symbols })?;
                    if class == 0 {
                        dc[id] = Some(decoder)
                    } else {
                        ac[id] = Some(decoder)
                    }
                }
            }
            DRI => restart = u16::from_be_bytes([d[0], d[1]]) as usize,
            0xC0..=0xC2 => {
                progressive = s.marker == 0xC2;
                if d.len() < 6 || d[0] != 8 {
                    return Err(unsupported());
                }
                jpeg.height = u16::from_be_bytes([d[1], d[2]]) as usize;
                jpeg.width = u16::from_be_bytes([d[3], d[4]]) as usize;
                let n = d[5] as usize;
                if jpeg.width == 0 || jpeg.height == 0 || n == 0 || n > 4 || d.len() < 6 + 3 * n {
                    return Err(unsupported());
                }
                for i in 0..n {
                    let c = &d[6 + 3 * i..9 + 3 * i];
                    let (h, v) = (c[1] >> 4, c[1] & 15);
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) || c[2] > 3 {
                        return Err(bad("frame header"));
                    }
                    jpeg.components.push(Component {
                        id: c[0],
                        h,
                        v,
                        tq: c[2],
                        bw: 0,
                        bh: 0,
                        blocks: Vec::new(),
                    });
                }
                jpeg.allocate();
                seen_frame = true;
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(unsupported()),
            SOS => {
                if !seen_frame || d.is_empty() {
                    return Err(bad("scan before frame"));
                }
                let n = d[0] as usize;
                if d.len() < 1 + 2 * n + 3 {
                    return Err(bad("scan header"));
                }
                let mut comps = Vec::new();
                for i in 0..n {
                    let (id, tables) = (d[1 + 2 * i], d[2 + 2 * i]);
                    let index = jpeg
                        .components
                        .iter()
                        .position(|c| c.id == id)
                        .ok_or_else(|| bad("scan component"))?;
                    comps.push((
                        index,
                        (tables >> 4) as usize & 3,
                        (tables & 15) as usize & 3,
                    ));
                }
                let t = &d[1 + 2 * n..];
                let scan = Scan {
                    comps,
                    ss: t[0] as usize,
                    se: t[1] as usize,
                    ah: u32::from(t[2] >> 4),
                    al: u32::from(t[2] & 15),
                };
                if scan.se > 63
                    || scan.ss > scan.se
                    || (!progressive && (scan.ss != 0 || scan.se != 63))
                {
                    return Err(bad("scan header"));
                }
                decode_scan(&mut jpeg, &scan, s.scan, restart, &dc, &ac, progressive)?;
            }
            EOI => break,
            0xE0..=0xEF | markers::COM => jpeg.extras.push((s.marker, d.to_vec())),
            _ => {}
        }
    }
    if !seen_frame {
        return Err(bad("no frame"));
    }
    Ok(jpeg)
}

fn table(t: &[Option<Decoder>; 4], i: usize) -> io::Result<&Decoder> {
    t[i].as_ref().ok_or_else(|| bad("missing Huffman table"))
}

fn decode_scan(
    jpeg: &mut Jpeg,
    scan: &Scan,
    data: &[u8],
    restart: usize,
    dc: &[Option<Decoder>; 4],
    ac: &[Option<Decoder>; 4],
    progressive: bool,
) -> io::Result<()> {
    let mut r = BitReader::new(data);
    let mut preds = vec![0i32; jpeg.components.len()];
    let mut eobrun = 0u32;
    // One block per MCU when the scan has one component; whole MCUs otherwise.
    let units: Vec<(usize, usize, usize)> = if scan.comps.len() == 1 {
        let ci = scan.comps[0].0;
        let (w, h) = jpeg.used_blocks(&jpeg.components[ci]);
        let bw = jpeg.components[ci].bw;
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (ci, y * bw + x, 0)))
            .collect()
    } else {
        let (mx, my) = jpeg.mcus();
        let mut v = Vec::new();
        for my_ in 0..my {
            for mx_ in 0..mx {
                for (k, &(ci, _, _)) in scan.comps.iter().enumerate() {
                    let c = &jpeg.components[ci];
                    for by in 0..c.v as usize {
                        for bx in 0..c.h as usize {
                            let x = mx_ * c.h as usize + bx;
                            let y = my_ * c.v as usize + by;
                            v.push((ci, y * c.bw + x, k + 1));
                        }
                    }
                }
            }
        }
        v
    };
    let blocks_per_mcu = if scan.comps.len() == 1 {
        1
    } else {
        scan.comps
            .iter()
            .map(|&(ci, _, _)| jpeg.components[ci].h as usize * jpeg.components[ci].v as usize)
            .sum()
    };
    for (i, &(ci, bi, _)) in units.iter().enumerate() {
        let mcu = i / blocks_per_mcu;
        if restart > 0 && mcu > 0 && mcu % restart == 0 && i % blocks_per_mcu == 0 {
            r.restart();
            preds.iter_mut().for_each(|p| *p = 0);
            eobrun = 0;
        }
        let k = scan.comps.iter().position(|c| c.0 == ci).unwrap_or(0);
        let (_, dct, act) = scan.comps[k];
        let block = &mut jpeg.components[ci].blocks[bi];
        if !progressive {
            let (dcd, acd) = (table(dc, dct)?, table(ac, act)?);
            let t = dcd.decode(&mut r)? as u32;
            preds[ci] += r.signed(t);
            block[0] = preds[ci] as i16;
            let mut k = 1;
            while k < 64 {
                let rs = acd.decode(&mut r)?;
                let (run, size) = ((rs >> 4) as usize, u32::from(rs & 15));
                if size == 0 {
                    if run != 15 {
                        break;
                    }
                    k += 16;
                    continue;
                }
                k += run;
                if k > 63 {
                    return Err(bad("coefficient overrun"));
                }
                block[ZIGZAG[k]] = r.signed(size) as i16;
                k += 1;
            }
        } else if scan.ss == 0 {
            if scan.ah == 0 {
                let t = table(dc, dct)?.decode(&mut r)? as u32;
                preds[ci] += r.signed(t);
                block[0] = (preds[ci] << scan.al) as i16;
            } else if r.bits(1) == 1 {
                block[0] |= 1 << scan.al;
            }
        } else if scan.ah == 0 {
            ac_first(block, scan, table(ac, act)?, &mut r, &mut eobrun)?;
        } else {
            ac_refine(block, scan, table(ac, act)?, &mut r, &mut eobrun)?;
        }
    }
    Ok(())
}

fn ac_first(
    block: &mut Block,
    scan: &Scan,
    acd: &Decoder,
    r: &mut BitReader,
    eobrun: &mut u32,
) -> io::Result<()> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = scan.ss;
    while k <= scan.se {
        let rs = acd.decode(r)?;
        let (run, size) = (u32::from(rs >> 4), u32::from(rs & 15));
        if size == 0 {
            if run < 15 {
                *eobrun = (1 << run) - 1 + r.bits(run);
                break;
            }
            k += 16;
            continue;
        }
        k += run as usize;
        if k > 63 {
            return Err(bad("coefficient overrun"));
        }
        block[ZIGZAG[k]] = (r.signed(size) << scan.al) as i16;
        k += 1;
    }
    Ok(())
}

fn ac_refine(
    block: &mut Block,
    scan: &Scan,
    acd: &Decoder,
    r: &mut BitReader,
    eobrun: &mut u32,
) -> io::Result<()> {
    let p1 = 1i16 << scan.al;
    let m1 = -1i16 << scan.al;
    let refine = |c: &mut i16, r: &mut BitReader| {
        if r.bits(1) == 1 && (*c & p1) == 0 {
            *c += if *c >= 0 { p1 } else { m1 };
        }
    };
    let mut k = scan.ss;
    if *eobrun == 0 {
        while k <= scan.se {
            let rs = acd.decode(r)?;
            let (mut run, size) = (i32::from(rs >> 4), rs & 15);
            let mut value = 0i16;
            if size != 0 {
                value = if r.bits(1) == 1 { p1 } else { m1 };
            } else if run != 15 {
                *eobrun = (1 << run) + r.bits(run as u32);
                break;
            }
            while k <= scan.se {
                let c = &mut block[ZIGZAG[k]];
                if *c != 0 {
                    refine(c, r);
                } else {
                    run -= 1;
                    if run < 0 {
                        break;
                    }
                }
                k += 1;
            }
            if value != 0 {
                if k > 63 {
                    return Err(bad("coefficient overrun"));
                }
                block[ZIGZAG[k]] = value;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= scan.se {
            let c = &mut block[ZIGZAG[k]];
            if *c != 0 {
                refine(c, r);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}
