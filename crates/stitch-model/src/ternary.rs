//! Explicit Prism legacy group128 codec. Never infer this layout from type42 alone.
//! Modern upstream type42 uses group64; older Prism files reused that identifier.
use anyhow::{ensure, Result};
pub const GROUP: usize = 128;
pub const BLOCK_BYTES: usize = 34;
fn scale(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = (bits >> 10) & 31;
    let fraction = bits & 1023;
    match exponent {
        0 => sign * (fraction as f32) * 2f32.powi(-24),
        31 => {
            if fraction == 0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + fraction as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15),
    }
}
pub fn matvec_rows(packed: &[u8], input: &[f32]) -> Result<Vec<f32>> {
    ensure!(
        !input.is_empty() && input.len() <= 65536 && input.len().is_multiple_of(GROUP),
        "invalid group128 projection dimensions"
    );
    ensure!(input.iter().all(|v| v.is_finite()), "non-finite input");
    let stride = input.len() / GROUP * BLOCK_BYTES;
    ensure!(
        !packed.is_empty()
            && packed.len().is_multiple_of(stride)
            && packed.len() <= 1024 * 1024 * 1024,
        "invalid packed matrix span"
    );
    let rows = packed.len() / stride;
    ensure!(rows <= 262144, "too many projection rows");
    let mut output = Vec::with_capacity(rows);
    for row in packed.chunks_exact(stride) {
        let mut sum = 0f32;
        for (block, x) in row
            .as_chunks::<BLOCK_BYTES>()
            .0
            .iter()
            .zip(input.as_chunks::<GROUP>().0.iter())
        {
            let d = scale(u16::from_le_bytes([block[0], block[1]]));
            ensure!(d.is_finite(), "non-finite packed scale");
            for (j, v) in x.iter().enumerate() {
                let q = ((block[2 + j / 4] >> ((j % 4) * 2)) & 3) as i32 - 1;
                sum += q as f32 * d * v;
            }
        }
        ensure!(sum.is_finite(), "projection overflow");
        output.push(sum);
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_codes_and_scale() {
        let mut b = vec![0u8; 34];
        b[..2].copy_from_slice(&0x3c00u16.to_le_bytes());
        b[2..].fill(0b11100100);
        let y = matvec_rows(&b, &vec![1.; 128]).unwrap();
        assert_eq!(y, vec![64.]);
    }
    #[test]
    fn bad_scale_and_shape_rejected() {
        assert!(matvec_rows(&[0; 34], &[1.; 64]).is_err());
        let mut b = [0; 34];
        b[..2].copy_from_slice(&0x7e00u16.to_le_bytes());
        assert!(matvec_rows(&b, &[1.; 128]).is_err());
    }
}
