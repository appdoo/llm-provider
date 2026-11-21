use super::*;
use crate::agent::macro_support::output;

/// F2: a panic in the work is `ToolError::Panicked`, which the `#[tool]`
/// result mapping marks as an error; the pool still runs the next work.
#[tokio::test]
async fn panicking_work_is_panicked_and_the_pool_survives() {
    let error = blocking(|| -> Result<(), ToolError> { panic!("boom") })
        .await
        .unwrap_err();
    assert!(matches!(error, ToolError::Panicked(_)));
    assert_eq!(error.to_string(), "the file tool panicked");

    let out = output(Err::<String, _>(error));
    assert!(out.is_error);
    assert_eq!(out.content, "the file tool panicked");

    let next = blocking(|| Ok(1)).await.unwrap();
    assert_eq!(next, 1);
}
