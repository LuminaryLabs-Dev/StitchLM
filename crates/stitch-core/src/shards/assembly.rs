use super::definition::*;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceResult {
    pub contributor: String,
    pub range: Range,
    pub values: Vec<f32>,
    pub checksum: String,
}
fn check_parts(parts: &[SliceResult], required: &[String]) -> Result<()> {
    let ids: BTreeSet<_> = parts.iter().map(|p| &p.contributor).collect();
    ensure!(
        parts.len() == ids.len() && ids == required.iter().collect(),
        "missing, duplicate or unexpected contributor"
    );
    for p in parts {
        p.range.length()?;
        ensure!(
            p.values.iter().all(|v| v.is_finite())
                && digest(&super::execution::float_bytes(&p.values)) == p.checksum,
            "partial-result integrity failed"
        );
    }
    Ok(())
}
pub fn concatenate(parts: &[SliceResult], required: &[String], total: Range) -> Result<Vec<f32>> {
    check_parts(parts, required)?;
    let length = total.length()?;
    ensure!(length <= 4_194_304, "assembly output bound exceeded");
    let mut sorted: Vec<_> = parts.iter().collect();
    sorted.sort_by_key(|p| p.range.start);
    let mut cursor = total.start;
    let mut out = Vec::with_capacity(length as usize);
    for p in sorted {
        ensure!(
            p.range.start == cursor
                && p.range.end <= total.end
                && p.values.len() as u64 == p.range.length()?,
            "gap, overlap or wrong slice length"
        );
        cursor = p.range.end;
        out.extend_from_slice(&p.values);
    }
    ensure!(cursor == total.end, "incomplete assembly");
    Ok(out)
}
pub fn sum_reduction(
    parts: &[SliceResult],
    required: &[String],
    reduction: Range,
    output_len: usize,
) -> Result<Vec<f32>> {
    check_parts(parts, required)?;
    ensure!(
        output_len > 0 && output_len <= 4_194_304,
        "invalid reduction output length"
    );
    let mut sorted: Vec<_> = parts.iter().collect();
    sorted.sort_by_key(|p| p.range.start);
    let mut cursor = reduction.start;
    let mut sum = vec![0f64; output_len];
    for p in sorted {
        ensure!(
            p.range.start == cursor && p.range.end <= reduction.end && p.values.len() == output_len,
            "reduction coverage/shape mismatch"
        );
        cursor = p.range.end;
        for (a, b) in sum.iter_mut().zip(&p.values) {
            *a += f64::from(*b);
        }
    }
    ensure!(cursor == reduction.end, "missing reduction span");
    let output: Vec<f32> = sum.into_iter().map(|v| v as f32).collect();
    ensure!(output.iter().all(|v| v.is_finite()), "reduction overflow");
    Ok(output)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionPartial {
    pub contributor: String,
    pub key_range: Range,
    pub maximum: f64,
    pub exp_sum: f64,
    pub weighted_values: Vec<f64>,
}
pub fn attention_partial(
    id: &str,
    start: u64,
    logits: &[f32],
    values: &[Vec<f32>],
) -> Result<AttentionPartial> {
    ensure!(
        !logits.is_empty() && logits.len() == values.len() && logits.iter().all(|v| v.is_finite()),
        "invalid attention segment"
    );
    let width = values.first().context("empty values")?.len();
    ensure!(
        width > 0
            && values
                .iter()
                .all(|r| r.len() == width && r.iter().all(|v| v.is_finite())),
        "invalid value matrix"
    );
    let maximum = logits
        .iter()
        .map(|x| f64::from(*x))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut exp_sum = 0.;
    let mut weighted = vec![0.; width];
    for (logit, row) in logits.iter().zip(values) {
        let w = (f64::from(*logit) - maximum).exp();
        exp_sum += w;
        for (out, v) in weighted.iter_mut().zip(row) {
            *out += w * f64::from(*v);
        }
    }
    Ok(AttentionPartial {
        contributor: id.into(),
        key_range: Range {
            start,
            end: start
                .checked_add(logits.len() as u64)
                .context("key range overflow")?,
        },
        maximum,
        exp_sum,
        weighted_values: weighted,
    })
}
pub fn merge_attention(
    parts: &[AttentionPartial],
    required: &[String],
    total: Range,
) -> Result<Vec<f32>> {
    ensure!(!parts.is_empty(), "no attention contributions");
    let ids: BTreeSet<_> = parts.iter().map(|p| &p.contributor).collect();
    ensure!(
        ids.len() == parts.len() && ids == required.iter().collect(),
        "attention contributors mismatch"
    );
    let width = parts[0].weighted_values.len();
    ensure!(width > 0 && width <= 65536, "invalid attention width");
    let mut sorted: Vec<_> = parts.iter().collect();
    sorted.sort_by_key(|p| p.key_range.start);
    let mut cursor = total.start;
    let maximum = parts
        .iter()
        .map(|p| p.maximum)
        .fold(f64::NEG_INFINITY, f64::max);
    ensure!(maximum.is_finite(), "invalid attention maximum");
    let mut denominator = 0.;
    let mut output = vec![0f64; width];
    for p in sorted {
        ensure!(
            p.key_range.start == cursor
                && p.key_range.end > cursor
                && p.key_range.end <= total.end
                && p.maximum.is_finite()
                && p.exp_sum.is_finite()
                && p.exp_sum > 0.
                && p.weighted_values.len() == width
                && p.weighted_values.iter().all(|v| v.is_finite()),
            "invalid attention partial/coverage"
        );
        cursor = p.key_range.end;
        let scale = (p.maximum - maximum).exp();
        denominator += p.exp_sum * scale;
        for (o, v) in output.iter_mut().zip(&p.weighted_values) {
            *o += v * scale;
        }
    }
    ensure!(
        cursor == total.end && denominator > 0. && denominator.is_finite(),
        "incomplete attention normalization"
    );
    let output: Vec<f32> = output
        .into_iter()
        .map(|v| (v / denominator) as f32)
        .collect();
    ensure!(
        output.iter().all(|v| v.is_finite()),
        "attention output overflow"
    );
    Ok(output)
}
