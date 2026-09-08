use std::ffi::OsString;

#[derive(Debug, Eq, PartialEq)]
pub struct EnqueueOptions {
    pub file_id: String,
    pub profile: String,
}

/// Parse a preview enqueue request.
///
/// # Errors
///
/// Returns an error for missing, unknown, duplicate, or malformed options.
pub fn parse_enqueue_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<EnqueueOptions, String> {
    let mut file_id = None;
    let mut profile = "web".to_owned();
    let mut profile_seen = false;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        let name = argument.to_string_lossy();
        let value = arguments
            .next()
            .ok_or_else(|| format!("{name} requires a value"))?
            .to_string_lossy()
            .into_owned();
        match name.as_ref() {
            "--file" if file_id.is_none() => file_id = Some(value),
            "--profile" if !profile_seen => {
                profile = value;
                profile_seen = true;
            }
            "--file" | "--profile" => return Err(format!("duplicate option: {name}")),
            _ => return Err(format!("unknown option: {name}")),
        }
    }
    let file_id = file_id.ok_or_else(|| "--file is required".to_owned())?;
    if profile != "web" && profile != "fine" {
        return Err("--profile must be web or fine".to_owned());
    }
    Ok(EnqueueOptions { file_id, profile })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(values: &'a [&'a str]) -> impl Iterator<Item = OsString> + 'a {
        values.iter().map(OsString::from)
    }

    #[test]
    fn enqueue_defaults_to_web_and_accepts_fine() {
        assert_eq!(
            parse_enqueue_options(args(&["--file", "file-id"])).expect("default request"),
            EnqueueOptions {
                file_id: "file-id".to_owned(),
                profile: "web".to_owned(),
            }
        );
        assert_eq!(
            parse_enqueue_options(args(&["--file", "file-id", "--profile", "fine"]))
                .expect("fine request")
                .profile,
            "fine"
        );
    }

    #[test]
    fn enqueue_rejects_incomplete_or_invalid_requests() {
        assert!(parse_enqueue_options(args(&[])).is_err());
        assert!(parse_enqueue_options(args(&["--file", "file-id", "--profile", "huge"])).is_err());
        assert!(parse_enqueue_options(args(&["--wat", "value"])).is_err());
    }
}
