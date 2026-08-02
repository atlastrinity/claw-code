//! Fractal transcript compaction using δ-governed geometric thinning.

use super::constants::FEIGENBAUM_DELTA;

/// Compact a list of message strings using geometric (δ-governed) thinning.
///
/// Recent messages up to `recent_count` are kept verbatim.
/// Older messages are kept at geometrically increasing intervals.
pub fn fractal_compact_messages(messages: &mut Vec<String>, total_capacity: usize) {
    if messages.len() <= total_capacity {
        return;
    }
    let recent_count = ((total_capacity as f64) / FEIGENBAUM_DELTA).floor() as usize;
    let recent_count = recent_count.max(1);
    
    let split_pos = messages.len().saturating_sub(recent_count);
    let recent: Vec<String> = messages.drain(split_pos..).collect();
    let older = std::mem::take(messages);
    
    let mut kept = Vec::new();
    let mut step = 1.0f64;
    let mut idx = (older.len() as f64) - 1.0;
    
    while idx >= 0.0 {
        let i = idx as usize;
        if i < older.len() {
            kept.push(older[i].clone());
        }
        step *= FEIGENBAUM_DELTA;
        idx -= step;
    }
    
    kept.reverse();
    kept.extend(recent);
    *messages = kept;
}
