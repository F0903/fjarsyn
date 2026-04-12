pub fn truncate(s: &str, len: usize) -> &str {
    match s.char_indices().nth(len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

pub fn truncate_with_ellipsis(s: &str, len: usize) -> String {
    if s.chars().count() <= len { s.to_string() } else { format!("{}...", truncate(s, len)) }
}

pub fn abbreviate_middle(s: &str, prefix_len: usize, suffix_len: usize) -> String {
    let total_chars = s.chars().count();
    if total_chars <= prefix_len + suffix_len + 3 {
        return s.to_string();
    }

    let prefix = truncate(s, prefix_len);
    let suffix = s.chars().skip(total_chars.saturating_sub(suffix_len)).collect::<String>();
    format!("{}...{}", prefix, suffix)
}
