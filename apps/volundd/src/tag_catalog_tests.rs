use super::*;

#[test]
fn tag_identity_uses_nfkc_whitespace_case_and_length() {
    let (display, normalized) = normalize("  ＣoreXY\u{2003}Printer ").unwrap();
    assert_eq!(display, "CoreXY Printer");
    assert_eq!(normalized, "corexy printer");
    assert!(normalize(&"x".repeat(51)).is_err());
}
