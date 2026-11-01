//! Real LFM block execution, shared by native and WASM. No network or UI here.
pub mod lfm2;
use anyhow::{ensure, Context, Result};
use candle::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use lfm2::{Config, Lfm2Config};
use safetensors::SafeTensors;
use sha2::{Digest, Sha256};
use stitch_core::{BlockRange, TensorFrame, MAX_CONTEXT};
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn resolve_config(json: &str, weights: &[u8], context: usize) -> Result<Config> {
    ensure!(
        (8..=MAX_CONTEXT).contains(&context),
        "context outside supported bound"
    );
    let mut v: serde_json::Value = serde_json::from_str(json)?;
    if let Some(theta) = v.pointer("/rope_parameters/rope_theta").cloned() {
        v["rope_theta"] = theta;
    }
    let raw: Lfm2Config = serde_json::from_value(v)?;
    let mut cfg = raw.into_config(false);
    cfg.max_position_embeddings = context;
    // Actual published tensor shape is authoritative. LFM auto-adjusted FFN dimensions
    // differ from both raw intermediate_size and the upstream helper's computed default.
    let tensors = SafeTensors::deserialize(weights)?;
    let first = tensors.tensor("model.layers.0.feed_forward.w1.weight")?;
    ensure!(
        first.shape().len() == 2 && first.shape()[1] == cfg.hidden_size,
        "invalid FFN tensor"
    );
    cfg.intermediate_size = first.shape()[0];
    validate_config(&cfg)?;
    for i in 0..cfg.num_hidden_layers {
        for name in ["w1", "w3"] {
            ensure!(
                tensors
                    .tensor(&format!("model.layers.{i}.feed_forward.{name}.weight"))?
                    .shape()
                    == [cfg.intermediate_size, cfg.hidden_size],
                "inconsistent FFN shape"
            );
        }
        ensure!(
            tensors
                .tensor(&format!("model.layers.{i}.feed_forward.w2.weight"))?
                .shape()
                == [cfg.hidden_size, cfg.intermediate_size],
            "invalid down projection"
        );
    }
    Ok(cfg)
}
pub fn validate_config(cfg: &Config) -> Result<()> {
    ensure!(
        cfg.hidden_size > 0
            && cfg.hidden_size <= 4096
            && cfg.num_hidden_layers > 0
            && cfg.num_hidden_layers <= 64,
        "unsupported model size"
    );
    ensure!(
        cfg.num_attention_heads > 0
            && cfg.num_key_value_heads > 0
            && cfg.hidden_size.is_multiple_of(cfg.num_attention_heads)
            && cfg
                .num_attention_heads
                .is_multiple_of(cfg.num_key_value_heads),
        "invalid attention heads"
    );
    ensure!(
        cfg.intermediate_size > 0
            && cfg.intermediate_size <= 32768
            && cfg.vocab_size > 0
            && cfg.vocab_size <= 262144,
        "unsupported model dimensions"
    );
    ensure!(
        cfg.layer_types.len() == cfg.num_hidden_layers
            && (8..=MAX_CONTEXT).contains(&cfg.max_position_embeddings)
            && cfg.conv_l_cache <= 16,
        "invalid model state contract"
    );
    ensure!(
        cfg.norm_eps.is_finite()
            && cfg.norm_eps > 0.0
            && cfg.rope_theta.is_finite()
            && cfg.rope_theta > 0.0
            && !cfg.use_flash_attn,
        "unsupported numerical configuration"
    );
    Ok(())
}
pub fn frame(t: &Tensor) -> Result<TensorFrame> {
    let (b, s, h) = t.dims3()?;
    Ok(TensorFrame {
        shape: [b, s, h],
        values: t.flatten_all()?.to_vec1::<f32>()?,
    })
}
pub fn tensor(f: &TensorFrame, hidden: usize, context: usize) -> Result<Tensor> {
    f.validate(hidden, context)?;
    Ok(Tensor::from_vec(f.values.clone(), &f.shape, &Device::Cpu)?)
}
pub fn shard_bytes(weights: &[u8], range: &BlockRange, layers: usize) -> Result<Vec<u8>> {
    range.validate(layers)?;
    let data = SafeTensors::deserialize(weights)?;
    let mut selected = Vec::new();
    for name in data.names() {
        if (range.start..range.end).any(|i| name.starts_with(&format!("model.layers.{i}."))) {
            selected.push((name.to_string(), data.tensor(name)?));
        }
    }
    ensure!(!selected.is_empty(), "empty shard");
    Ok(safetensors::tensor::serialize(selected, None)?)
}
pub fn load_model(weights: &[u8], cfg: &Config) -> Result<lfm2::Model> {
    validate_config(cfg)?;
    let vb = VarBuilder::from_slice_safetensors(weights, DType::F32, &Device::Cpu)?;
    lfm2::Model::new(cfg, vb).context("load LFM model")
}
pub fn load_shard(weights: &[u8], cfg: &Config, range: &BlockRange) -> Result<lfm2::BlockShard> {
    validate_config(cfg)?;
    range.validate(cfg.num_hidden_layers)?;
    let vb = VarBuilder::from_slice_safetensors(weights, DType::F32, &Device::Cpu)?;
    Ok(lfm2::BlockShard::new(cfg, range.start, range.end, vb)?)
}
pub fn new_cache(cfg: &Config) -> Result<lfm2::Cache> {
    Ok(lfm2::Cache::new(true, DType::F32, cfg, &Device::Cpu)?)
}

pub mod gguf;

pub mod ternary;

pub mod bonsai;
