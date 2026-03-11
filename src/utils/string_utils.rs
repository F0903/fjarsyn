pub fn truncate(s: &str, len: usize) -> &str {
    match s.char_indices().nth(len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
