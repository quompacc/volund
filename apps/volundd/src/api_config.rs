use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:8080";
const DEFAULT_DERIVED_ROOT: &str = "/srv/volund/derived";
const DEFAULT_WEB_ROOT: &str = "/usr/share/volund/web";
const DEFAULT_SUPPORT_ROOT: &str = "/srv/volund/support";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiConfig {
    pub listen_address: SocketAddr,
    pub derived_root: PathBuf,
    pub web_root: PathBuf,
    pub support_root: PathBuf,
}

impl ApiConfig {
    /// Load the HTTP listen address from `VOLUND_LISTEN_ADDR`.
    ///
    /// The default is loopback-only so a fresh installation is not exposed to
    /// the network before an operator configures a reverse proxy or firewall.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured value is not an IP socket address.
    pub fn from_environment() -> Result<Self, String> {
        let value = env::var("VOLUND_LISTEN_ADDR").ok();
        let derived_root = env::var_os("VOLUND_DERIVED_ROOT")
            .map_or_else(|| PathBuf::from(DEFAULT_DERIVED_ROOT), PathBuf::from);
        let web_root = env::var_os("VOLUND_WEB_ROOT")
            .map_or_else(|| PathBuf::from(DEFAULT_WEB_ROOT), PathBuf::from);
        let support_root = env::var_os("VOLUND_SUPPORT_ROOT")
            .map_or_else(|| PathBuf::from(DEFAULT_SUPPORT_ROOT), PathBuf::from);
        Self::from_values(value.as_deref(), derived_root, web_root, support_root)
    }

    fn from_values(
        value: Option<&str>,
        derived_root: PathBuf,
        web_root: PathBuf,
        support_root: PathBuf,
    ) -> Result<Self, String> {
        let value = value.unwrap_or(DEFAULT_LISTEN_ADDRESS);
        let listen_address = SocketAddr::from_str(value)
            .map_err(|error| format!("invalid VOLUND_LISTEN_ADDR: {error}"))?;
        if !derived_root.to_string_lossy().starts_with('/') {
            return Err("VOLUND_DERIVED_ROOT must be an absolute Unix path".to_owned());
        }
        if !web_root.to_string_lossy().starts_with('/') {
            return Err("VOLUND_WEB_ROOT must be an absolute Unix path".to_owned());
        }
        if !support_root.to_string_lossy().starts_with('/') {
            return Err("VOLUND_SUPPORT_ROOT must be an absolute Unix path".to_owned());
        }
        Ok(Self {
            listen_address,
            derived_root,
            web_root,
            support_root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_listener_is_loopback_only() {
        let config = ApiConfig::from_values(
            None,
            DEFAULT_DERIVED_ROOT.into(),
            DEFAULT_WEB_ROOT.into(),
            DEFAULT_SUPPORT_ROOT.into(),
        )
        .expect("default");
        assert_eq!(config.listen_address.to_string(), DEFAULT_LISTEN_ADDRESS);
        assert!(config.listen_address.ip().is_loopback());
    }

    #[test]
    fn explicit_listener_must_be_a_socket_address() {
        let config = ApiConfig::from_values(
            Some("0.0.0.0:9080"),
            "/derived".into(),
            "/web".into(),
            "/support".into(),
        )
        .expect("explicit config");
        assert_eq!(config.listen_address.to_string(), "0.0.0.0:9080");
        assert!(
            ApiConfig::from_values(
                Some("localhost:8080"),
                "/derived".into(),
                "/web".into(),
                "/support".into(),
            )
            .is_err()
        );
        assert!(
            ApiConfig::from_values(None, "relative".into(), "/web".into(), "/support".into())
                .is_err()
        );
        assert!(
            ApiConfig::from_values(
                None,
                "/derived".into(),
                "relative".into(),
                "/support".into()
            )
            .is_err()
        );
        assert!(
            ApiConfig::from_values(None, "/derived".into(), "/web".into(), "relative".into())
                .is_err()
        );
    }
}
