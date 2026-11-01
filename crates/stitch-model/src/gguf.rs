//! Bounded GGUF inspection. Container readability never implies kernel support.
use anyhow::{ensure, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};
const MAX_HEADER: u64 = 128 * 1024 * 1024;
const MAX_ITEMS: u64 = 1_000_000;
const MAX_STRING: u64 = 16 * 1024 * 1024;
struct Reader<'a, R> {
    r: &'a mut R,
    used: u64,
}
impl<R: Read> Reader<'_, R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.used = self
            .used
            .checked_add(N as u64)
            .ok_or_else(|| anyhow::anyhow!("header overflow"))?;
        ensure!(self.used <= MAX_HEADER, "GGUF header exceeds128MiB");
        let mut b = [0; N];
        self.r.read_exact(&mut b)?;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u64()?;
        ensure!(
            n <= MAX_STRING && self.used.checked_add(n).is_some_and(|x| x <= MAX_HEADER),
            "GGUF string exceeds bounds"
        );
        let mut b = vec![0; n as usize];
        self.r.read_exact(&mut b)?;
        self.used += n;
        Ok(String::from_utf8(b)?)
    }
    fn value(&mut self, kind: u32, depth: usize, keep: bool) -> Result<Value> {
        ensure!(depth <= 2, "nested metadata arrays exceed limit");
        Ok(match kind {
            0 => json!(u8::from_le_bytes(self.bytes()?)),
            1 => json!(i8::from_le_bytes(self.bytes()?)),
            2 => json!(u16::from_le_bytes(self.bytes()?)),
            3 => json!(i16::from_le_bytes(self.bytes()?)),
            4 => json!(self.u32()?),
            5 => json!(i32::from_le_bytes(self.bytes()?)),
            6 => {
                let v = f32::from_le_bytes(self.bytes()?);
                ensure!(v.is_finite(), "non-finite metadata");
                json!(v)
            }
            7 => {
                let v = self.bytes::<1>()?[0];
                ensure!(v <= 1, "invalid bool");
                json!(v == 1)
            }
            8 => {
                let s = self.string()?;
                if keep {
                    json!(s)
                } else {
                    Value::Null
                }
            }
            9 => {
                let t = self.u32()?;
                let n = self.u64()?;
                ensure!(n <= MAX_ITEMS, "metadata array too long");
                for _ in 0..n {
                    self.value(t, depth + 1, false)?;
                }
                json!({"array_type":t,"length":n})
            }
            10 => json!(self.u64()?),
            11 => json!(i64::from_le_bytes(self.bytes()?)),
            12 => {
                let v = f64::from_le_bytes(self.bytes()?);
                ensure!(v.is_finite(), "non-finite metadata");
                json!(v)
            }
            _ => anyhow::bail!("unknown GGUF metadata type{kind}"),
        })
    }
}
#[derive(Debug, Serialize)]
pub struct TensorInfo {
    pub name: String,
    pub dimensions: Vec<u64>,
    pub quant_type: u32,
    pub offset: u64,
    pub storage_span_bytes: u64,
    pub f32_expansion_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub version: u32,
    pub file_bytes: u64,
    pub data_offset: u64,
    pub architecture: Option<String>,
    pub metadata: BTreeMap<String, Value>,
    pub tensors: Vec<TensorInfo>,
    pub quant_types: BTreeMap<u32, usize>,
    pub block_storage_bytes: BTreeMap<usize, u64>,
    pub full_model_inference_supported: bool,
    pub reason: String,
}
pub fn inspect<R: Read + Seek>(file: &mut R) -> Result<Inspection> {
    let size = file.seek(SeekFrom::End(0))?;
    file.rewind()?;
    let mut r = Reader { r: file, used: 0 };
    ensure!(&r.bytes::<4>()? == b"GGUF", "not a GGUF container");
    let version = r.u32()?;
    ensure!(
        (2..=3).contains(&version),
        "unsupported GGUF version{version}"
    );
    let tensor_count = r.u64()?;
    let kv_count = r.u64()?;
    ensure!(
        tensor_count > 0 && tensor_count <= MAX_ITEMS && kv_count <= MAX_ITEMS,
        "GGUF item count exceeds bounds"
    );
    let mut metadata = BTreeMap::new();
    for _ in 0..kv_count {
        let key = r.string()?;
        ensure!(
            key.len() <= 4096 && !metadata.contains_key(&key),
            "invalid or duplicate metadata key"
        );
        let kind = r.u32()?;
        let value = r.value(kind, 0, true)?;
        metadata.insert(key, value);
    }
    let alignment = metadata
        .get("general.alignment")
        .and_then(Value::as_u64)
        .unwrap_or(32);
    ensure!(
        alignment.is_power_of_two() && alignment <= 4096,
        "invalid alignment"
    );
    let mut tensors = Vec::new();
    let mut names = std::collections::HashSet::new();
    let mut quant_types = BTreeMap::new();
    for _ in 0..tensor_count {
        let name = r.string()?;
        ensure!(
            !name.is_empty() && name.len() <= 4096 && names.insert(name.clone()),
            "invalid or duplicate tensor name"
        );
        let count = r.u32()?;
        ensure!((1..=4).contains(&count), "invalid tensor rank");
        let mut dims = Vec::new();
        let mut elements = 1u64;
        for _ in 0..count {
            let n = r.u64()?;
            ensure!(n > 0, "zero tensor dimension");
            elements = elements
                .checked_mul(n)
                .ok_or_else(|| anyhow::anyhow!("tensor size overflow"))?;
            dims.push(n);
        }
        let quant_type = r.u32()?;
        let offset = r.u64()?;
        ensure!(offset % alignment == 0, "unaligned tensor offset");
        *quant_types.entry(quant_type).or_default() += 1;
        let f32_expansion_bytes = elements
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("tensor byte overflow"))?;
        tensors.push(TensorInfo {
            name,
            dimensions: dims,
            quant_type,
            offset,
            storage_span_bytes: 0,
            f32_expansion_bytes,
        });
    }
    let data_offset = r
        .used
        .checked_add(alignment - 1)
        .ok_or_else(|| anyhow::anyhow!("offset overflow"))?
        / alignment
        * alignment;
    ensure!(data_offset < size, "no tensor payload");
    tensors.sort_by_key(|t| t.offset);
    for i in 0..tensors.len() {
        let next = if i + 1 < tensors.len() {
            tensors[i + 1].offset
        } else {
            size - data_offset
        };
        ensure!(
            tensors[i].offset < next && next <= size - data_offset,
            "overlapping or out-of-file tensor span"
        );
        tensors[i].storage_span_bytes = next - tensors[i].offset;
    }
    let mut blocks = BTreeMap::new();
    for t in &tensors {
        if let Some(index) = t
            .name
            .strip_prefix("blk.")
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<usize>().ok())
        {
            *blocks.entry(index).or_default() += t.storage_span_bytes;
        }
    }
    let architecture = metadata
        .get("general.architecture")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Inspection{version,file_bytes:size,data_offset,architecture,metadata,tensors,quant_types,block_storage_bytes:blocks,full_model_inference_supported:false,reason:"GGUF inspection does not authorize execution. This build's distributed backend is validated for pinned LFM safetensors; GGUF architecture, quantization and activation transforms require an explicit tested backend.".into()})
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn malformed_headers_fail_without_allocating_payload() {
        for b in [vec![], b"GGUF".to_vec(), b"NOPEabcdefgh".to_vec()] {
            assert!(inspect(&mut Cursor::new(b)).is_err());
        }
    }
    #[test]
    fn huge_count_rejected() {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        assert!(inspect(&mut Cursor::new(b)).is_err());
    }
}
