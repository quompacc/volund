//! Stable domain rules shared by VÖLUND services.

use std::fmt;
use std::str::FromStr;

pub const CAD_CONVERT_CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CadFormat {
    Step,
    Iges,
    Brep,
    Stl,
    ThreeMf,
    Obj,
    Ply,
    Gltf,
    Glb,
}

impl CadFormat {
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "step" | "stp" => Some(Self::Step),
            "iges" | "igs" => Some(Self::Iges),
            "brep" => Some(Self::Brep),
            "stl" => Some(Self::Stl),
            "3mf" => Some(Self::ThreeMf),
            "obj" => Some(Self::Obj),
            "ply" => Some(Self::Ply),
            "gltf" => Some(Self::Gltf),
            "glb" => Some(Self::Glb),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Iges => "iges",
            Self::Brep => "brep",
            Self::Stl => "stl",
            Self::ThreeMf => "3mf",
            Self::Obj => "obj",
            Self::Ply => "ply",
            Self::Gltf => "gltf",
            Self::Glb => "glb",
        }
    }
}

impl fmt::Display for CadFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentHash(String);

impl ContentHash {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ContentHash {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 {
            return Err("SHA-256 must contain exactly 64 hexadecimal characters");
        }
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("SHA-256 contains a non-hexadecimal character");
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}

/// Validate and normalize a path stored relative to a configured source root.
///
/// Absolute paths, parent traversal, and platform-specific prefixes are refused
/// so persisted metadata can never escape or depend on one host's mount point.
///
/// # Errors
///
/// Returns an error for empty paths, absolute paths, Windows separators or
/// prefixes, empty segments, and `.` or `..` traversal components.
pub fn normalize_library_path(path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("library path is empty");
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("library path must use relative, forward-slash notation");
    }

    for component in path.split('/') {
        match component {
            "" => return Err("library path contains an empty component"),
            "." => return Err("library path contains a current-directory component"),
            ".." => return Err("library path contains parent traversal"),
            _ => {}
        }
    }
    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_normalized_to_lowercase() {
        let input = "A".repeat(64);
        let hash = ContentHash::from_str(&input).expect("valid hash");
        assert_eq!(hash.as_str(), "a".repeat(64));
    }

    #[test]
    fn malformed_content_hashes_are_rejected() {
        assert!(ContentHash::from_str("abc").is_err());
        assert!(ContentHash::from_str(&"z".repeat(64)).is_err());
    }

    #[test]
    fn cad_extensions_are_classified_case_insensitively() {
        assert_eq!(CadFormat::from_extension("STEP"), Some(CadFormat::Step));
        assert_eq!(CadFormat::from_extension("stp"), Some(CadFormat::Step));
        assert_eq!(CadFormat::from_extension("igs"), Some(CadFormat::Iges));
        assert_eq!(CadFormat::from_extension("3MF"), Some(CadFormat::ThreeMf));
        assert_eq!(CadFormat::from_extension("txt"), None);
        assert_eq!(CadFormat::Glb.to_string(), "glb");
    }

    #[test]
    fn library_paths_are_relative_and_cannot_escape() {
        assert_eq!(
            normalize_library_path("assemblies/drive.step"),
            Ok("assemblies/drive.step".to_owned())
        );
        assert!(normalize_library_path("../secret.step").is_err());
        assert!(normalize_library_path("C:\\secret.step").is_err());
        assert!(normalize_library_path("/secret.step").is_err());
    }
}
