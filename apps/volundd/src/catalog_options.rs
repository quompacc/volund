use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Eq, PartialEq)]
pub struct RegisterRootOptions {
    pub key: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ScanOptions {
    pub root_key: String,
    pub full: bool,
}

/// Parse an immutable library-root registration request.
///
/// # Errors
///
/// Returns an error for missing, duplicate, unknown, malformed, or non-Unix
/// path arguments.
pub fn parse_register_root_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<RegisterRootOptions, String> {
    let pairs = parse_named_arguments(arguments, false)?;
    reject_unknown(&pairs, &["--key", "--name", "--path"])?;
    let key = required(&pairs, "--key")?;
    let name = required(&pairs, "--name")?;
    let raw_path = required(&pairs, "--path")?;
    let path = PathBuf::from(&raw_path);
    if !valid_root_key(&key) {
        return Err("--key must start with a lowercase letter and contain only lowercase letters, digits, '_' or '-' (maximum 63 characters)".to_owned());
    }
    if name.is_empty() {
        return Err("--name must not be empty".to_owned());
    }
    if !raw_path.starts_with('/') {
        return Err("--path must be an absolute Unix path".to_owned());
    }
    Ok(RegisterRootOptions { key, name, path })
}

/// Parse an incremental or full library scan request.
///
/// # Errors
///
/// Returns an error for missing, duplicate, unknown, or malformed arguments.
pub fn parse_scan_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<ScanOptions, String> {
    let pairs = parse_named_arguments(arguments, true)?;
    reject_unknown(&pairs, &["--root", "--full"])?;
    let root_key = required(&pairs, "--root")?;
    if !valid_root_key(&root_key) {
        return Err("--root is not a valid library key".to_owned());
    }
    Ok(ScanOptions {
        root_key,
        full: pairs.iter().any(|(name, _)| name == "--full"),
    })
}

fn parse_named_arguments(
    arguments: impl Iterator<Item = OsString>,
    allow_full: bool,
) -> Result<Vec<(String, Option<String>)>, String> {
    let mut arguments = arguments;
    let mut parsed = Vec::new();
    while let Some(raw_name) = arguments.next() {
        let name = raw_name
            .into_string()
            .map_err(|_| "arguments must be valid UTF-8".to_owned())?;
        if allow_full && name == "--full" {
            parsed.push((name, None));
            continue;
        }
        if !name.starts_with("--") {
            return Err(format!("unexpected argument: {name}"));
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {name}"))?
            .into_string()
            .map_err(|_| format!("value for {name} must be valid UTF-8"))?;
        if parsed.iter().any(|(existing, _)| existing == &name) {
            return Err(format!("duplicate argument: {name}"));
        }
        parsed.push((name, Some(value)));
    }
    Ok(parsed)
}

fn required(arguments: &[(String, Option<String>)], name: &str) -> Result<String, String> {
    arguments
        .iter()
        .find(|(candidate, _)| candidate == name)
        .and_then(|(_, value)| value.clone())
        .ok_or_else(|| format!("missing required argument: {name}"))
}

fn reject_unknown(arguments: &[(String, Option<String>)], allowed: &[&str]) -> Result<(), String> {
    if let Some((name, _)) = arguments
        .iter()
        .find(|(name, _)| !allowed.contains(&name.as_str()))
    {
        return Err(format!("unknown argument: {name}"));
    }
    Ok(())
}

fn valid_root_key(value: &str) -> bool {
    value.len() <= 63
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(values: &'a [&'a str]) -> impl Iterator<Item = OsString> + 'a {
        values.iter().map(OsString::from)
    }

    #[test]
    fn registration_requires_valid_complete_metadata() {
        let options = parse_register_root_options(args(&[
            "--key",
            "cad",
            "--name",
            "CAD Library",
            "--path",
            "/srv/volund/library",
        ]))
        .expect("valid options");
        assert_eq!(options.key, "cad");
        assert_eq!(options.path, PathBuf::from("/srv/volund/library"));
        assert!(parse_register_root_options(args(&["--key", "CAD"])).is_err());
    }

    #[test]
    fn scan_supports_an_explicit_full_rehash() {
        assert_eq!(
            parse_scan_options(args(&["--root", "cad", "--full"])).expect("valid options"),
            ScanOptions {
                root_key: "cad".to_owned(),
                full: true,
            }
        );
        assert!(parse_scan_options(args(&["--root", "cad", "--wat", "value"])).is_err());
    }
}
