use std::path::Path;

use crate::arch::{Role, Tensor, tensors};

pub const MAGIC: [u8; 8] = *b"PWSEFNET";
pub const FORMAT: u32 = 1;

pub(crate) const ALIGN: usize = 256;
const HEADER: usize = 16;
const ENTRY: usize = 16;

pub(crate) struct Weights {
    bytes: Vec<u8>,
    entries: Vec<(Tensor, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slice {
    pub(crate) offset: u64,
    pub(crate) size: u64,
}

impl Weights {
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(bytes)
    }

    pub(crate) fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        let expected = tensors();
        if bytes.len() < HEADER || bytes[..8] != MAGIC {
            return Err("not a weights file".into());
        }
        let format = u32::from_le_bytes(word(&bytes, 8));
        if format != FORMAT {
            return Err(format!("weights format {format}, expected {FORMAT}"));
        }
        let count = u32::from_le_bytes(word(&bytes, 12)) as usize;
        if count != expected.len() || bytes.len() < HEADER + count * ENTRY {
            return Err(format!("{count} tensors, expected {}", expected.len()));
        }
        let mut entries = Vec::with_capacity(count);
        for (ix, tensor) in expected.into_iter().enumerate() {
            let at = HEADER + ix * ENTRY;
            let offset = u64::from_le_bytes(long(&bytes, at)) as usize;
            let len = u64::from_le_bytes(long(&bytes, at + 8)) as usize;
            let fits = offset
                .checked_add(len * 4)
                .is_some_and(|end| end <= bytes.len());
            if len != tensor.len() || !offset.is_multiple_of(ALIGN) || !fits {
                return Err(format!(
                    "tensor {ix} ({:?}) does not match the network",
                    tensor.role
                ));
            }
            entries.push((tensor, offset));
        }
        Ok(Self { bytes, entries })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn slice(&self, role: Role) -> Slice {
        let (tensor, offset) = self
            .entries
            .iter()
            .find(|(t, _)| t.role == role)
            .unwrap_or_else(|| panic!("{role:?} is part of every weights file"));
        Slice {
            offset: *offset as u64,
            size: (tensor.len() * 4) as u64,
        }
    }

    pub(crate) fn floats(&self, role: Role) -> Vec<f32> {
        let slice = self.slice(role);
        let start = slice.offset as usize;
        self.bytes[start..start + slice.size as usize]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }
}

fn word(bytes: &[u8], at: usize) -> [u8; 4] {
    let mut out = [0; 4];
    out.copy_from_slice(&bytes[at..at + 4]);
    out
}

fn long(bytes: &[u8], at: usize) -> [u8; 8] {
    let mut out = [0; 8];
    out.copy_from_slice(&bytes[at..at + 8]);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    pub shape: [usize; 4],
}

pub fn expected() -> Vec<Expected> {
    tensors()
        .into_iter()
        .map(|t| Expected {
            shape: t.onnx_shape,
        })
        .collect()
}

pub fn encode(onnx: &[Vec<f32>]) -> Result<Vec<u8>, String> {
    let expected = tensors();
    if onnx.len() != expected.len() {
        return Err(format!(
            "{} tensors, expected {}",
            onnx.len(),
            expected.len()
        ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    bytes.extend_from_slice(&(expected.len() as u32).to_le_bytes());
    let table = bytes.len();
    bytes.resize(table + expected.len() * ENTRY, 0);
    for (ix, (tensor, values)) in expected.iter().zip(onnx).enumerate() {
        if values.len() != tensor.len() {
            return Err(format!(
                "tensor {ix} ({:?}) has {} values, expected {}",
                tensor.role,
                values.len(),
                tensor.len()
            ));
        }
        bytes.resize(bytes.len().next_multiple_of(ALIGN), 0);
        let offset = bytes.len();
        let at = table + ix * ENTRY;
        bytes[at..at + 8].copy_from_slice(&(offset as u64).to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&(values.len() as u64).to_le_bytes());
        let arranged = if tensor.transposed() {
            let [rows, cols, _, _] = tensor.onnx_shape;
            transpose(values, rows, cols)
        } else {
            values.clone()
        };
        for v in arranged {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    Ok(bytes)
}

fn transpose(values: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0f32; values.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = values[r * cols + c];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> Vec<Vec<f32>> {
        tensors()
            .iter()
            .enumerate()
            .map(|(ix, t)| (0..t.len()).map(|v| (ix * 1000 + v) as f32).collect())
            .collect()
    }

    #[test]
    fn an_encoded_file_reads_back() {
        let onnx = fake();
        let weights = Weights::parse(encode(&onnx).unwrap()).unwrap();
        assert_eq!(weights.floats(Role::StemBias), onnx[1]);
        assert_eq!(weights.floats(Role::ProbeRows), *onnx.last().unwrap());
        for (tensor, offset) in &weights.entries {
            assert_eq!(offset % ALIGN, 0, "{:?}", tensor.role);
        }
    }

    #[test]
    fn pointwise_weights_are_stored_input_major() {
        let onnx = fake();
        let weights = Weights::parse(encode(&onnx).unwrap()).unwrap();
        let ix = tensors()
            .iter()
            .position(|t| t.role == Role::ExpandWeight(1))
            .unwrap();
        let stored = weights.floats(Role::ExpandWeight(1));
        let [rows, cols, _, _] = tensors()[ix].onnx_shape;
        assert_eq!((rows, cols), (96, 16));
        assert_eq!(stored[1], onnx[ix][cols]);
        assert_eq!(stored[rows], onnx[ix][1]);
    }

    #[test]
    fn a_damaged_file_is_rejected() {
        let bytes = encode(&fake()).unwrap();
        assert!(Weights::parse(bytes[..bytes.len() - 4].to_vec()).is_err());
        let mut wrong = bytes.clone();
        wrong[0] = b'X';
        assert!(Weights::parse(wrong).is_err());
        let mut newer = bytes;
        newer[8] = 2;
        assert!(Weights::parse(newer).is_err());
        let mut short = fake();
        short[3].pop();
        assert!(encode(&short).is_err());
    }
}
