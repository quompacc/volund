use unicode_normalization::UnicodeNormalization;

/// Normalize a reusable metadata identity and preserve a clean display form.
///
/// # Errors
/// Returns an error when the collapsed value is empty or exceeds the bound.
pub fn normalize_identity(value: &str, max_chars: usize) -> Result<(String, String), String> {
    let display = value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let count = display.chars().count();
    if count == 0 || count > max_chars {
        return Err(format!(
            "metadata name must contain between 1 and {max_chars} characters"
        ));
    }
    let normalized = display.to_lowercase();
    Ok((display, normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_compatibility_case_and_whitespace_are_stable() {
        let (display, normalized) = normalize_identity("  ＶÖLUND\u{2003}Lab  ", 160).unwrap();
        assert_eq!(display, "VÖLUND Lab");
        assert_eq!(normalized, "völund lab");
        assert_eq!(normalize_identity("völund lab", 160).unwrap().1, normalized);
    }
}
