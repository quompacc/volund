use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::str::FromStr;

use sha2::{Digest, Sha256};
use volund_core::ContentHash;

/// Calculate the SHA-256 digest of a file through a bounded read buffer.
///
/// # Errors
///
/// Returns an error when the file cannot be opened/read or the resulting digest
/// cannot be represented by the domain type.
pub fn sha256_file(path: &Path) -> Result<ContentHash, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open {} for hashing: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    ContentHash::from_str(&format!("{digest:x}"))
        .map_err(|error| format!("cannot construct SHA-256: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn hashes_file_bytes_without_modifying_them() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("volund-hash-{unique}"));
        fs::write(&path, b"abc").expect("write fixture");
        let hash = sha256_file(&path).expect("hash fixture");
        assert_eq!(
            hash.as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(fs::read(&path).expect("read fixture"), b"abc");
        fs::remove_file(path).expect("remove fixture");
    }
}
