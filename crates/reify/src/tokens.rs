//! Token-cost estimation.
//!
//! Budgeting tolerates a ±10% error, so a heuristic is enough and keeps a tokenizer
//! dependency out of the hot path. Benchmarks that report token counts use the real
//! counts returned by the model provider, never this estimate — see `docs/PLAN.md` §Q.

/// Estimator identity, echoed in JSON output so a number can be traced to how it was counted.
pub const ESTIMATOR: &str = "heuristic-v1";

/// Approximate the number of model tokens `text` would occupy.
///
/// Latin script averages ~4 bytes per token; CJK averages closer to 1.5 characters per
/// token, so the two are counted separately rather than with one global divisor.
pub fn estimate(text: &str) -> u32 {
    let mut latin_bytes = 0usize;
    let mut wide_chars = 0usize;
    for ch in text.chars() {
        if is_wide_script(ch) {
            wide_chars += 1;
        } else {
            latin_bytes += ch.len_utf8();
        }
    }
    let latin = (latin_bytes as f32 / 4.0).ceil() as u32;
    let wide = (wide_chars as f32 / 1.5).ceil() as u32;
    (latin + wide).max(if text.is_empty() { 0 } else { 1 })
}

/// CJK ideographs, kana and Hangul, which tokenize far denser than Latin script.
fn is_wide_script(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF   // kana
        | 0x3400..=0x4DBF // CJK ext A
        | 0x4E00..=0x9FFF // CJK unified
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK compatibility
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_costs_nothing() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn any_non_empty_text_costs_at_least_one_token() {
        assert_eq!(estimate("a"), 1);
    }

    #[test]
    fn latin_text_is_about_four_bytes_per_token() {
        // 40 characters of ASCII should land near 10 tokens.
        let s = "a".repeat(40);
        assert_eq!(estimate(&s), 10);
    }

    #[test]
    fn cjk_costs_more_per_character_than_latin() {
        // Same character count, denser script must cost more.
        let cjk = estimate("承認が必要です承認が必要です");
        let latin = estimate("approval required xx");
        assert!(cjk > latin, "cjk {cjk} should exceed latin {latin}");
    }

    #[test]
    fn vietnamese_diacritics_are_counted_as_latin_bytes() {
        // Vietnamese is Latin script with multi-byte diacritics; it must not be
        // treated as a wide script or every budget on Vietnamese docs is wrong.
        let vi = estimate("khách hàng chiến lược");
        assert!((5..=12).contains(&vi), "unexpected vi estimate {vi}");
    }
}
