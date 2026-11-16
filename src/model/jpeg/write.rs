//! Writes coefficients back out as one sequential Huffman scan with tables built for this image
//! (what `jpegtran -optimize` does), which is how every lossless JPEG operation here ends.

use std::io;

use super::coeffs::{Block, Jpeg, ZIGZAG};
use super::huffman::{BitWriter, Encoder, Table, category, optimal};
use super::markers::{DHT, DQT, EOI, SOI, SOS, write_segment};

/// Runs `emit` over every block in scan order with the predictor kept per component.
fn walk(jpeg: &Jpeg, mut emit: impl FnMut(usize, &Block, i32)) {
    let mut preds = vec![0i32; jpeg.components.len()];
    let mut visit = |ci: usize, block: &Block| {
        let diff = i32::from(block[0]) - preds[ci];
        preds[ci] = i32::from(block[0]);
        emit(ci, block, diff);
    };
    if jpeg.components.len() == 1 {
        let c = &jpeg.components[0];
        let (w, h) = jpeg.used_blocks(c);
        for y in 0..h {
            for x in 0..w {
                visit(0, &c.blocks[y * c.bw + x]);
            }
        }
        return;
    }
    let (mx, my) = jpeg.mcus();
    for y in 0..my {
        for x in 0..mx {
            for (ci, c) in jpeg.components.iter().enumerate() {
                for by in 0..c.v as usize {
                    for bx in 0..c.h as usize {
                        let bi = (y * c.v as usize + by) * c.bw + x * c.h as usize + bx;
                        visit(ci, &c.blocks[bi]);
                    }
                }
            }
        }
    }
}

/// The (run, size) symbols and extra bits of one block's AC coefficients.
fn ac_symbols(block: &Block, mut out: impl FnMut(u8, u32, u32)) {
    let mut run = 0u8;
    for &z in &ZIGZAG[1..] {
        let v = i32::from(block[z]);
        if v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            out(0xF0, 0, 0);
            run -= 16;
        }
        let (size, bits) = category(v);
        out((run << 4) | size as u8, bits, size);
        run = 0;
    }
    if run > 0 {
        out(0x00, 0, 0);
    }
}

fn table_of(ci: usize) -> usize {
    usize::from(ci > 0)
}

/// The file for `jpeg`: SOI, its extras, tables, the frame and one scan.
pub fn write(jpeg: &Jpeg) -> io::Result<Vec<u8>> {
    let mut dc_freq = [[0u32; 256]; 2];
    let mut ac_freq = [[0u32; 256]; 2];
    let mut too_big = false;
    walk(jpeg, |ci, block, diff| {
        let t = table_of(ci);
        let (size, _) = category(diff);
        too_big |= size > 11;
        dc_freq[t][size as usize] += 1;
        ac_symbols(block, |s, _, size| {
            too_big |= size > 10;
            ac_freq[t][s as usize] += 1;
        });
    });
    if too_big {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "This JPEG has coefficients out of range",
        ));
    }
    let tables = if jpeg.components.len() == 1 { 1 } else { 2 };
    let dc_tables: Vec<Table> = (0..tables).map(|t| optimal(&dc_freq[t])).collect();
    let ac_tables: Vec<Table> = (0..tables).map(|t| optimal(&ac_freq[t])).collect();
    let dc_enc: Vec<Encoder> = dc_tables
        .iter()
        .map(Encoder::new)
        .collect::<io::Result<_>>()?;
    let ac_enc: Vec<Encoder> = ac_tables
        .iter()
        .map(Encoder::new)
        .collect::<io::Result<_>>()?;

    let mut w = BitWriter::default();
    walk(jpeg, |ci, block, diff| {
        let t = table_of(ci);
        let (size, bits) = category(diff);
        dc_enc[t].put(&mut w, size as u8);
        w.put(bits, size);
        ac_symbols(block, |s, bits, size| {
            ac_enc[t].put(&mut w, s);
            w.put(bits, size);
        });
    });
    let scan = w.finish();

    let mut out = Vec::with_capacity(scan.len() + 4096);
    out.extend_from_slice(&[0xFF, SOI]);
    for (marker, payload) in &jpeg.extras {
        write_segment(&mut out, *marker, payload);
    }
    let used: Vec<u8> = {
        let mut v: Vec<u8> = jpeg.components.iter().map(|c| c.tq).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let wide = used
        .iter()
        .any(|&t| jpeg.qtables[t as usize].iter().any(|&q| q > 255));
    for &t in &used {
        let mut p = vec![(u8::from(wide) << 4) | t];
        for &z in &ZIGZAG {
            let q = jpeg.qtables[t as usize][z];
            if wide {
                p.extend_from_slice(&q.to_be_bytes());
            } else {
                p.push(q as u8);
            }
        }
        write_segment(&mut out, DQT, &p);
    }
    let mut sof = vec![8];
    sof.extend_from_slice(&(jpeg.height as u16).to_be_bytes());
    sof.extend_from_slice(&(jpeg.width as u16).to_be_bytes());
    sof.push(jpeg.components.len() as u8);
    for c in &jpeg.components {
        sof.extend_from_slice(&[c.id, (c.h << 4) | c.v, c.tq]);
    }
    // 16-bit quantisation tables need the extended frame type.
    write_segment(&mut out, if wide { 0xC1 } else { 0xC0 }, &sof);
    let mut dht = Vec::new();
    for (class, list) in [(0u8, &dc_tables), (1u8, &ac_tables)] {
        for (id, t) in list.iter().enumerate() {
            dht.push((class << 4) | id as u8);
            dht.extend_from_slice(&t.counts);
            dht.extend_from_slice(&t.symbols);
        }
    }
    write_segment(&mut out, DHT, &dht);
    let mut sos = vec![jpeg.components.len() as u8];
    for (ci, c) in jpeg.components.iter().enumerate() {
        let t = table_of(ci) as u8;
        sos.extend_from_slice(&[c.id, (t << 4) | t]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    write_segment(&mut out, SOS, &sos);
    out.extend_from_slice(&scan);
    out.extend_from_slice(&[0xFF, EOI]);
    Ok(out)
}
