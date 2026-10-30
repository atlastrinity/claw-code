//! Fractal transcript sampling using δ-governed geometric thinning.

use super::constants::FEIGENBAUM_DELTA;

/// Indices selected by δ-geometric thinning over `len` items, sampled
/// backwards from the newest at intervals growing by δ.
#[must_use]
pub fn fractal_sample_indices(len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let mut picked = Vec::new();
    let mut step = 1.0f64;
    let mut idx = len as f64 - 1.0;
    while idx >= 0.0 {
        picked.push(idx as usize);
        step *= FEIGENBAUM_DELTA;
        idx -= step;
    }
    picked.reverse();
    picked
}
