//! Tests for the credential type: conversions and the masked `Debug`.

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
