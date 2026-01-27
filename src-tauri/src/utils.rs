//! Common utility functions for string manipulation and safe operations.

/// Safely truncate a string to at most `max_chars` characters.
///
/// Unlike byte slicing (`&s[..n]`), this respects UTF-8 character boundaries
/// and won't panic on multi-byte characters (e.g., emoji, CJK text).
///
/// If the string is longer than `max_chars`, it's truncated and "..." is appended.
/// The total length will be `max_chars` characters (including the ellipsis).
///
/// # Examples
///
/// ```
/// assert_eq!(truncate_str("Hello, World!", 10), "Hello, ...");
/// assert_eq!(truncate_str("Short", 10), "Short");
/// assert_eq!(truncate_str("日本語テスト", 5), "日本...");
/// ```
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        // Reserve 3 chars for "..."
        let truncate_at = max_chars.saturating_sub(3);
        let truncated: String = s.chars().take(truncate_at).collect();
        format!("{}...", truncated)
    }
}

/// Safely get a prefix of a string up to `max_chars` characters without ellipsis.
///
/// Useful when you need to slice a string for processing but don't want to add "...".
/// Respects UTF-8 character boundaries.
pub fn safe_prefix(s: &str, max_chars: usize) -> &str {
    if s.is_empty() {
        return s;
    }

    // Find the byte index of the nth character
    let byte_idx = s
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());

    &s[..byte_idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("Hello", 10), "Hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("Hello", 5), "Hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("Hello, World!", 10), "Hello, ...");
    }

    #[test]
    fn test_truncate_str_unicode() {
        // Japanese text - each character is 3 bytes in UTF-8
        assert_eq!(truncate_str("日本語テスト", 5), "日本...");
    }

    #[test]
    fn test_truncate_str_emoji() {
        // Emoji are 4 bytes each in UTF-8
        assert_eq!(truncate_str("🎉🎊🎁🎈🎂", 4), "🎉...");
    }

    #[test]
    fn test_safe_prefix() {
        assert_eq!(safe_prefix("Hello, World!", 5), "Hello");
        assert_eq!(safe_prefix("日本語", 2), "日本");
        assert_eq!(safe_prefix("short", 100), "short");
    }
}
