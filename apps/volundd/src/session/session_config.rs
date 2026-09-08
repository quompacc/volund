use super::{SessionError, SessionPolicy};

const SECURE_COOKIES_ENV: &str = "VOLUND_SECURE_COOKIES";
const INVALID_SECURE_COOKIE: &str = "VOLUND_SECURE_COOKIES must be true or false";

#[derive(Clone, Copy, Default)]
pub struct SessionConfig {
    policy: SessionPolicy,
    secure_cookies: bool,
}

impl SessionConfig {
    /// Load cookie security mode and reject unsafe non-loopback defaults.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid booleans or non-loopback operation without
    /// explicitly enabled secure cookies.
    pub fn from_environment(listener_is_loopback: bool) -> Result<Self, SessionError> {
        let value = match std::env::var(SECURE_COOKIES_ENV) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(SessionError::Unavailable(format!(
                    "cannot read {SECURE_COOKIES_ENV}: {error}"
                )));
            }
        };
        Self::from_cookie_value(listener_is_loopback, value.as_deref())
    }

    fn from_cookie_value(
        listener_is_loopback: bool,
        value: Option<&str>,
    ) -> Result<Self, SessionError> {
        let secure_cookies = match value {
            Some("true") => true,
            Some("false") | None => false,
            Some(_) => return Err(SessionError::Unavailable(INVALID_SECURE_COOKIE.to_owned())),
        };
        if !listener_is_loopback && !secure_cookies {
            return Err(SessionError::Unavailable(
                "non-loopback listeners require VOLUND_SECURE_COOKIES=true".to_owned(),
            ));
        }
        Ok(Self {
            policy: SessionPolicy::default(),
            secure_cookies,
        })
    }

    #[must_use]
    pub fn policy(self) -> SessionPolicy {
        self.policy
    }

    #[must_use]
    pub fn secure_cookies(self) -> bool {
        self.secure_cookies
    }
}

#[cfg(test)]
mod tests {
    use super::SessionConfig;

    #[test]
    fn non_loopback_listener_requires_secure_cookie_mode() {
        assert!(SessionConfig::from_cookie_value(true, None).is_ok());
        assert!(SessionConfig::from_cookie_value(true, Some("false")).is_ok());
        assert!(SessionConfig::from_cookie_value(false, None).is_err());
        assert!(SessionConfig::from_cookie_value(false, Some("false")).is_err());
        let secure = SessionConfig::from_cookie_value(false, Some("true"))
            .unwrap_or_else(|_| panic!("secure non-loopback configuration rejected"));
        assert!(secure.secure_cookies());
        assert!(SessionConfig::from_cookie_value(true, Some("TRUE")).is_err());
        assert!(SessionConfig::from_cookie_value(true, Some("1")).is_err());
    }
}
