//! Split file text into overlapping windows (character-based UTF-8).

#[must_use]
pub fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }
    let overlap = overlap.min(max_chars.saturating_sub(1));
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return out;
    }
    let mut start = 0;
    loop {
        let mut end = (start + max_chars).min(chars.len());
        
        // Smart Chunking: Try to find a newline or space to break cleanly
        if end < chars.len() {
            let mut newline_pos = end;
            let min_acceptable_end = start + (max_chars / 2);
            while newline_pos > min_acceptable_end && chars[newline_pos - 1] != '\n' {
                newline_pos -= 1;
            }
            if newline_pos > min_acceptable_end {
                end = newline_pos;
            } else {
                let mut space_pos = end;
                while space_pos > min_acceptable_end && chars[space_pos - 1] != ' ' {
                    space_pos -= 1;
                }
                if space_pos > min_acceptable_end {
                    end = space_pos;
                }
            }
        }
        
        let piece: String = chars[start..end].iter().collect();
        if !piece.trim().is_empty() {
            out.push(piece);
        }
        if end >= chars.len() {
            break;
        }
        let step = end.saturating_sub(overlap).max(start + 1);
        start = step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_non_empty() {
        let c = chunk_text("hello world test", 5, 2);
        assert!(!c.is_empty());
        let joined: String = c.join("");
        assert!(joined.contains("hello"));
    }
}
