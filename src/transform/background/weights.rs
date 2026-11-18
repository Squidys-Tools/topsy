//! Reads a `.safetensors` file (an 8-byte header length, a JSON header, raw little-endian data)
//! and hands out its f32 tensors by name, folding BatchNorms into the convolutions before them.

use std::collections::HashMap;
use std::io;

use serde::Deserialize;

use super::deform::DeformConv;
use super::ops::{Conv, Linear};

#[derive(Deserialize)]
struct Entry {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

pub struct Weights {
    tensors: HashMap<String, (Vec<usize>, Vec<f32>)>,
}

fn bad(what: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("The background model is damaged ({what})"),
    )
}

impl Weights {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let len =
            u64::from_le_bytes(*bytes.first_chunk::<8>().ok_or_else(|| bad("short"))?) as usize;
        let header = bytes.get(8..8 + len).ok_or_else(|| bad("header"))?;
        let entries: HashMap<String, serde_json::Value> =
            serde_json::from_slice(header).map_err(bad)?;
        let data = &bytes[8 + len..];
        let mut tensors = HashMap::new();
        for (name, value) in entries {
            if name == "__metadata__" {
                continue;
            }
            let e: Entry = serde_json::from_value(value).map_err(bad)?;
            if e.dtype != "F32" {
                continue;
            }
            let raw = data
                .get(e.data_offsets[0]..e.data_offsets[1])
                .ok_or_else(|| bad(&name))?;
            let values = raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect::<Vec<_>>();
            if values.len() != e.shape.iter().product::<usize>() {
                return Err(bad(&name));
            }
            tensors.insert(name, (e.shape, values));
        }
        Ok(Self { tensors })
    }

    pub fn take(&mut self, name: &str) -> io::Result<(Vec<usize>, Vec<f32>)> {
        self.tensors
            .remove(name)
            .ok_or_else(|| bad(format!("no {name}")))
    }

    pub fn vec(&mut self, name: &str) -> io::Result<Vec<f32>> {
        Ok(self.take(name)?.1)
    }

    pub fn linear(&mut self, prefix: &str, bias: bool) -> io::Result<Linear> {
        let (shape, weight) = self.take(&format!("{prefix}.weight"))?;
        let bias = if bias {
            Some(self.vec(&format!("{prefix}.bias"))?)
        } else {
            None
        };
        Ok(Linear {
            out: shape[0],
            inp: shape[1],
            weight,
            bias,
        })
    }

    /// A convolution; a missing bias is zero.
    pub fn conv(&mut self, prefix: &str, stride: usize, pad: usize) -> io::Result<Conv> {
        let (shape, weight) = self.take(&format!("{prefix}.weight"))?;
        let bias = match self.take(&format!("{prefix}.bias")) {
            Ok((_, b)) => b,
            Err(_) => vec![0.0; shape[0]],
        };
        Ok(Conv {
            out: shape[0],
            inp: shape[1],
            k: shape[2],
            stride,
            pad,
            weight,
            bias,
        })
    }

    /// A same-size convolution followed by the BatchNorm at `bn`, folded together.
    pub fn conv_bn(&mut self, prefix: &str, bn: &str) -> io::Result<Conv> {
        let mut conv = self.conv(prefix, 1, 0)?;
        conv.pad = conv.k / 2;
        let gamma = self.vec(&format!("{bn}.weight"))?;
        let beta = self.vec(&format!("{bn}.bias"))?;
        let mean = self.vec(&format!("{bn}.running_mean"))?;
        let var = self.vec(&format!("{bn}.running_var"))?;
        conv.fold_bn(&gamma, &beta, &mean, &var);
        Ok(conv)
    }

    /// `DeformableConv2d` at `prefix`, with the BatchNorm at `bn` folded into its sampling conv.
    pub fn deform(&mut self, prefix: &str, bn: &str) -> io::Result<DeformConv> {
        let mut offsets = self.conv(&format!("{prefix}.offset_conv"), 1, 0)?;
        let gate = self.conv(&format!("{prefix}.modulator_conv"), 1, 0)?;
        offsets.pad = offsets.k / 2;
        offsets.out += gate.out;
        offsets.weight.extend(gate.weight);
        offsets.bias.extend(gate.bias);
        let conv = self.conv_bn(&format!("{prefix}.regular_conv"), bn)?;
        Ok(DeformConv { offsets, conv })
    }
}
