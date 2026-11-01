//! Text-only Qwen3.5/Bonsai execution. The host owns all recurrent and KV state.
//! Matrix multiplication is injected so local streaming and remote shards share
//! the exact layer graph. Only the pinned legacy group-128 artifact is admitted.
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;

pub const WEIGHTS_SHA256: &str = "527f276ddf047b3494de964985b5529b9fc3ecf739ea64d8fab3a6ef8403e81d";
pub const HIDDEN: usize = 5120;
pub const LAYERS: usize = 64;
pub const VOCAB: usize = 248320;
pub const EPS: f32 = 1e-6;

pub trait Weights {
    fn vector(&mut self, name: &str) -> Result<Vec<f32>>;
    fn embedding(&mut self, token: u32) -> Result<Vec<f32>>;
    fn matvec(&mut self, name: &str, input: &[f32]) -> Result<Vec<f32>>;
    fn observe(&mut self, _name: &str, _values: &[f32]) -> Result<()> {
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub enum LayerState {
    Linear {
        convolution: Vec<f32>,
        recurrent: Vec<f32>,
    },
    Attention {
        keys: Vec<f32>,
        values: Vec<f32>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct State {
    pub model_sha256: String,
    pub position: usize,
    pub context_limit: usize,
    pub layers: Vec<LayerState>,
}
impl State {
    pub fn new(context_limit: usize) -> Result<Self> {
        ensure!(
            (1..=512).contains(&context_limit),
            "Bonsai context must be 1..512"
        );
        Ok(Self {
            model_sha256: WEIGHTS_SHA256.into(),
            position: 0,
            context_limit,
            layers: (0..LAYERS)
                .map(|i| {
                    if (i + 1) % 4 == 0 {
                        LayerState::Attention {
                            keys: Vec::new(),
                            values: Vec::new(),
                        }
                    } else {
                        LayerState::Linear {
                            convolution: vec![0.; 3 * 10240],
                            recurrent: vec![0.; 48 * 128 * 128],
                        }
                    }
                })
                .collect(),
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.model_sha256 == WEIGHTS_SHA256
                && self.layers.len() == LAYERS
                && self.context_limit <= 512
                && self.context_limit > 0
                && self.position <= self.context_limit,
            "Bonsai state identity or bounds invalid"
        );
        for (i, s) in self.layers.iter().enumerate() {
            match s {
                LayerState::Linear {
                    convolution,
                    recurrent,
                } => ensure!(
                    (i + 1) % 4 != 0
                        && convolution.len() == 3 * 10240
                        && recurrent.len() == 48 * 128 * 128
                        && convolution.iter().chain(recurrent).all(|v| v.is_finite()),
                    "invalid recurrent state"
                ),
                LayerState::Attention { keys, values } => ensure!(
                    (i + 1) % 4 == 0
                        && keys.len() == self.position * 1024
                        && values.len() == keys.len()
                        && keys.iter().chain(values).all(|v| v.is_finite()),
                    "invalid KV state"
                ),
            }
        }
        Ok(())
    }
}
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn sigmoid(x: f32) -> f32 {
    if x >= 0. {
        1. / (1. + (-x).exp())
    } else {
        let e = x.exp();
        e / (1. + e)
    }
}
fn silu(x: f32) -> f32 {
    x * sigmoid(x)
}
fn softplus(x: f32) -> f32 {
    if x > 20. {
        x
    } else {
        x.exp().ln_1p()
    }
}
fn rms(x: &[f32], weight: &[f32]) -> Result<Vec<f32>> {
    ensure!(
        x.len() == weight.len() && !x.is_empty(),
        "normalization shape mismatch"
    );
    let scale = (dot(x, x) / x.len() as f32 + EPS).sqrt().recip();
    Ok(x.iter().zip(weight).map(|(v, w)| v * scale * w).collect())
}
fn l2(x: &mut [f32]) {
    let squared = x.iter().map(|v| (*v * *v) as f64).sum::<f64>();
    let scale = (squared.sqrt() as f32).max(EPS).recip();
    for v in x {
        *v *= scale;
    }
}
fn rope(x: &mut [f32], position: usize) {
    // Text has identical temporal/height/width positions. MRoPE therefore reduces
    // to the same partial split-half rotation for all three section assignments.
    for i in 0..32 {
        let angle = position as f32 * 10_000_000f32.powf(-((2 * i) as f32) / 64.);
        let (c, s) = (angle.cos(), angle.sin());
        let (a, b) = (x[i], x[i + 32]);
        x[i] = a * c - b * s;
        x[i + 32] = a * s + b * c;
    }
}
fn add(a: &mut [f32], b: &[f32]) -> Result<()> {
    ensure!(a.len() == b.len(), "residual shape mismatch");
    for (x, y) in a.iter_mut().zip(b) {
        *x += y;
    }
    Ok(())
}
fn linear<W: Weights>(
    w: &mut W,
    p: &str,
    x: &[f32],
    conv: &mut [f32],
    state: &mut [f32],
) -> Result<Vec<f32>> {
    let qkv = w.matvec(&format!("{p}.attn_qkv.weight"), x)?;
    let z = w.matvec(&format!("{p}.attn_gate.weight"), x)?;
    let beta = w.matvec(&format!("{p}.ssm_beta.weight"), x)?;
    let alpha = w.matvec(&format!("{p}.ssm_alpha.weight"), x)?;
    let bias = w.vector(&format!("{p}.ssm_dt.bias"))?;
    let a = w.vector(&format!("{p}.ssm_a"))?;
    let kernel = w.vector(&format!("{p}.ssm_conv1d.weight"))?;
    ensure!(
        qkv.len() == 10240
            && z.len() == 6144
            && beta.len() == 48
            && alpha.len() == 48
            && bias.len() == 48
            && a.len() == 48
            && kernel.len() == 4 * 10240,
        "linear attention projection shape mismatch"
    );
    let mut convolved = vec![0.; 10240];
    for c in 0..10240 {
        let j = c * 3;
        let k = c * 4;
        convolved[c] = silu(
            conv[j] * kernel[k]
                + conv[j + 1] * kernel[k + 1]
                + conv[j + 2] * kernel[k + 2]
                + qkv[c] * kernel[k + 3],
        );
        conv[j] = conv[j + 1];
        conv[j + 1] = conv[j + 2];
        conv[j + 2] = qkv[c];
    }
    for head in 0..32 {
        l2(&mut convolved[head * 128..(head + 1) * 128]);
    }
    w.observe(&format!("{p}.conv_normalized"), &convolved)?;
    let norm = w.vector(&format!("{p}.ssm_norm.weight"))?;
    ensure!(norm.len() == 128, "SSM norm mismatch");
    let mut output = vec![0.; 6144];
    for h in 0..48 {
        let q = &convolved[(h % 16) * 128..(h % 16 + 1) * 128];
        let k = &convolved[2048 + (h % 16) * 128..2048 + (h % 16 + 1) * 128];
        let v = &convolved[4096 + h * 128..4096 + (h + 1) * 128];
        let decay = (a[h] * softplus(alpha[h] + bias[h])).exp();
        let b = sigmoid(beta[h]);
        let st = &mut state[h * 128 * 128..(h + 1) * 128 * 128];
        for item in st.iter_mut() {
            *item *= decay;
        }
        let mut y = vec![0.; 128];
        for j in 0..128 {
            let row = &mut st[j * 128..(j + 1) * 128];
            let delta = (v[j] - dot(row, k)) * b;
            for i in 0..128 {
                row[i] += k[i] * delta;
            }
            y[j] = dot(row, q) / 128f32.sqrt();
        }
        let y = rms(&y, &norm)?;
        for j in 0..128 {
            output[h * 128 + j] = y[j] * silu(z[h * 128 + j]);
        }
    }
    w.observe(&format!("{p}.linear_output"), &output)?;
    w.matvec(&format!("{p}.ssm_out.weight"), &output)
}
fn attention<W: Weights>(
    w: &mut W,
    p: &str,
    x: &[f32],
    position: usize,
    keys: &mut Vec<f32>,
    values: &mut Vec<f32>,
) -> Result<Vec<f32>> {
    let qg = w.matvec(&format!("{p}.attn_q.weight"), x)?;
    let mut k = w.matvec(&format!("{p}.attn_k.weight"), x)?;
    let v = w.matvec(&format!("{p}.attn_v.weight"), x)?;
    ensure!(
        qg.len() == 12288 && k.len() == 1024 && v.len() == 1024,
        "full attention projection shape mismatch"
    );
    let qnorm = w.vector(&format!("{p}.attn_q_norm.weight"))?;
    let knorm = w.vector(&format!("{p}.attn_k_norm.weight"))?;
    for h in 0..4 {
        let n = rms(&k[h * 256..(h + 1) * 256], &knorm)?;
        k[h * 256..(h + 1) * 256].copy_from_slice(&n);
        rope(&mut k[h * 256..(h + 1) * 256], position);
    }
    keys.extend(k);
    values.extend(v);
    let mut out = vec![0.; 6144];
    for h in 0..24 {
        let mut q = rms(&qg[h * 512..h * 512 + 256], &qnorm)?;
        rope(&mut q, position);
        let kh = h / 6;
        let mut scores = (0..=position)
            .map(|t| dot(&q, &keys[t * 1024 + kh * 256..t * 1024 + (kh + 1) * 256]) / 16.)
            .collect::<Vec<_>>();
        let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut total = 0.;
        for s in &mut scores {
            *s = (*s - max).exp();
            total += *s;
        }
        for t in 0..=position {
            let scale = scores[t] / total;
            for j in 0..256 {
                out[h * 256 + j] += scale * values[t * 1024 + kh * 256 + j];
            }
        }
        for j in 0..256 {
            out[h * 256 + j] *= sigmoid(qg[h * 512 + 256 + j]);
        }
    }
    w.matvec(&format!("{p}.attn_output.weight"), &out)
}
/// The caller must discard or restore state after any failed/cancelled token.
/// A successful return alone advances the committed position.
pub fn forward<W: Weights>(w: &mut W, token: u32, state: &mut State) -> Result<Vec<f32>> {
    state.validate()?;
    ensure!(
        token < VOCAB as u32
            && state.model_sha256 == WEIGHTS_SHA256
            && state.layers.len() == LAYERS
            && state.position < state.context_limit,
        "invalid token or exhausted Bonsai context"
    );
    let mut x = w.embedding(token)?;
    ensure!(x.len() == HIDDEN, "embedding shape mismatch");
    w.observe("embedding", &x)?;
    for i in 0..LAYERS {
        let p = format!("blk.{i}");
        let norm = w.vector(&format!("{p}.attn_norm.weight"))?;
        let n = rms(&x, &norm)?;
        let branch = match &mut state.layers[i] {
            LayerState::Linear {
                convolution,
                recurrent,
            } => linear(w, &p, &n, convolution, recurrent)?,
            LayerState::Attention { keys, values } => {
                attention(w, &p, &n, state.position, keys, values)?
            }
        };
        add(&mut x, &branch)?;
        w.observe(&format!("{p}.attn_residual"), &x)?;
        let norm = w.vector(&format!("{p}.post_attention_norm.weight"))?;
        let n = rms(&x, &norm)?;
        let gate = w.matvec(&format!("{p}.ffn_gate.weight"), &n)?;
        let up = w.matvec(&format!("{p}.ffn_up.weight"), &n)?;
        ensure!(
            gate.len() == 17408 && up.len() == 17408,
            "FFN shape mismatch"
        );
        let hidden = gate
            .iter()
            .zip(&up)
            .map(|(g, u)| silu(*g) * u)
            .collect::<Vec<_>>();
        let down = w.matvec(&format!("{p}.ffn_down.weight"), &hidden)?;
        add(&mut x, &down)?;
        ensure!(
            x.iter().all(|v| v.is_finite()),
            "non-finite Bonsai hidden state"
        );
        w.observe(&format!("{p}.output"), &x)?;
    }
    let norm = w.vector("output_norm.weight")?;
    let x = rms(&x, &norm)?;
    let logits = w.matvec("output.weight", &x)?;
    ensure!(
        logits.len() == VOCAB && logits.iter().all(|v| v.is_finite()),
        "invalid logits"
    );
    state.position += 1;
    Ok(logits)
}

#[cfg(not(target_arch = "wasm32"))]
pub struct FileWeights {
    pub file: std::fs::File,
    pub inspection: crate::gguf::Inspection,
    vectors: BTreeMap<String, Vec<f32>>,
    pub working_bytes: usize,
    pub bytes_read: u64,
    pub trace_directory: Option<std::path::PathBuf>,
}
#[cfg(not(target_arch = "wasm32"))]
impl FileWeights {
    pub fn open(path: &std::path::Path, working_bytes: usize) -> Result<Self> {
        use sha2::{Digest, Sha256};
        use std::io::{Read, Seek};
        ensure!(
            (1_000_000..=64_000_000).contains(&working_bytes),
            "working buffer must be 1..64 MB"
        );
        let mut file = std::fs::File::open(path)?;
        ensure!(
            file.metadata()?.len() == 7165121600,
            "unexpected Bonsai artifact size"
        );
        let mut hash = Sha256::new();
        let mut b = [0u8; 65536];
        loop {
            let n = file.read(&mut b)?;
            if n == 0 {
                break;
            }
            hash.update(&b[..n]);
        }
        ensure!(
            hex::encode(hash.finalize()) == WEIGHTS_SHA256,
            "Bonsai artifact checksum mismatch"
        );
        file.rewind()?;
        let inspection = crate::gguf::inspect(&mut file)?;
        ensure!(
            inspection.architecture.as_deref() == Some("qwen35") && inspection.tensors.len() == 851,
            "unexpected model layout"
        );
        Ok(Self {
            file,
            inspection,
            vectors: BTreeMap::new(),
            working_bytes,
            bytes_read: 0,
            trace_directory: None,
        })
    }
    pub fn tensor(&self, name: &str) -> Result<&crate::gguf::TensorInfo> {
        self.inspection
            .tensors
            .iter()
            .find(|t| t.name == name)
            .with_context(|| format!("tensor missing: {name}"))
    }
    pub fn read_range(&mut self, name: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let t = self.tensor(name)?;
        ensure!(
            offset
                .checked_add(length as u64)
                .is_some_and(|n| n <= t.storage_span_bytes),
            "tensor read exceeds bounds"
        );
        let start = self.inspection.data_offset + t.offset + offset;
        self.file.seek(SeekFrom::Start(start))?;
        let mut b = vec![0; length];
        self.file.read_exact(&mut b)?;
        self.bytes_read += length as u64;
        Ok(b)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Weights for FileWeights {
    fn vector(&mut self, name: &str) -> Result<Vec<f32>> {
        if let Some(v) = self.vectors.get(name) {
            return Ok(v.clone());
        }
        let t = self.tensor(name)?;
        ensure!(
            t.quant_type == 0 && t.storage_span_bytes <= 2_000_000,
            "unsupported vector encoding"
        );
        let bytes = t.dimensions.iter().product::<u64>() as usize * 4;
        let b = self.read_range(name, 0, bytes)?;
        let v = b
            .as_chunks::<4>()
            .0
            .iter()
            .map(|x| f32::from_le_bytes(*x))
            .collect::<Vec<_>>();
        ensure!(v.iter().all(|x| x.is_finite()), "non-finite vector");
        self.vectors.insert(name.into(), v.clone());
        Ok(v)
    }
    fn embedding(&mut self, token: u32) -> Result<Vec<f32>> {
        ensure!(token < VOCAB as u32, "token out of range");
        let stride = HIDDEN / 128 * 34;
        let b = self.read_range("token_embd.weight", token as u64 * stride as u64, stride)?;
        crate::ternary::decode(&b)
    }
    fn matvec(&mut self, name: &str, input: &[f32]) -> Result<Vec<f32>> {
        let t = self.tensor(name)?;
        ensure!(
            t.quant_type == 42
                && t.dimensions.len() == 2
                && t.dimensions[0] as usize == input.len()
                && input.len().is_multiple_of(128),
            "matrix layout mismatch"
        );
        let rows = t.dimensions[1] as usize;
        let stride = input.len() / 128 * 34;
        let chunk = (self.working_bytes / stride).max(1);
        let mut output = Vec::with_capacity(rows);
        for start in (0..rows).step_by(chunk) {
            let count = chunk.min(rows - start);
            let b = self.read_range(name, (start * stride) as u64, count * stride)?;
            output.extend(crate::ternary::matvec_rows(&b, input)?);
        }
        Ok(output)
    }
    fn observe(&mut self, name: &str, values: &[f32]) -> Result<()> {
        if let Some(dir) = &self.trace_directory {
            let b = values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            std::fs::write(dir.join(format!("{name}.f32")), b)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_normalization_and_rope_are_finite() {
        let mut x = vec![0.; 128];
        l2(&mut x);
        assert!(x.iter().all(|v| *v == 0.));
        assert_eq!(rms(&x, &vec![1.; 128]).unwrap(), x);
    }
    #[test]
    fn model_state_identity_is_checked() {
        let s = State {
            model_sha256: "wrong".into(),
            position: 0,
            context_limit: 1,
            layers: vec![],
        };
        assert!(s.validate().is_err());
    }
    #[test]
    fn stable_gates() {
        assert_eq!(sigmoid(1000.), 1.);
        assert_eq!(sigmoid(-1000.), 0.);
        assert_eq!(softplus(1000.), 1000.);
        assert!(softplus(-1000.).is_finite());
    }
    #[test]
    fn rope_preserves_norm() {
        let mut x = (0..256).map(|i| (i as f32 * 0.1).sin()).collect::<Vec<_>>();
        let old = dot(&x, &x);
        let tail = x[64..].to_vec();
        rope(&mut x, 127);
        assert!((old - dot(&x, &x)).abs() < 1e-4);
        assert_eq!(tail, x[64..]);
    }
    #[test]
    fn context_bounds() {
        assert!(State::new(0).is_err());
        assert!(State::new(513).is_err());
    }
}
