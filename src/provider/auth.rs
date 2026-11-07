//! Credential types passed in by the caller.

/// A credential supplied by the caller.
#[derive(Clone)]
#[non_exhaustive]
pub enum AuthMethod {
    /// API key sent via `x-api-key` header.
    ApiKey(String),
    /// OAuth access token (sk-ant-oat01-*) sent via `Authorization: Bearer` header.
    Bearer(String),
}

/// Hand-written so the secret never reaches logs.
impl std::fmt::Debug for AuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthMethod::ApiKey(_) => f.debug_tuple("ApiKey").field(&"***").finish(),
            AuthMethod::Bearer(_) => f.debug_tuple("Bearer").field(&"***").finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Debug output masks both credential kinds.
    #[test]
    fn debug_masks_secret() {
        let key = format!("{:?}", AuthMethod::ApiKey("sk-secret".into()));
        let bearer = format!("{:?}", AuthMethod::Bearer("oat-secret".into()));
        assert_eq!(key, r#"ApiKey("***")"#);
        assert_eq!(bearer, r#"Bearer("***")"#);
        assert!(!key.contains("secret") && !bearer.contains("secret"));
    }
}
