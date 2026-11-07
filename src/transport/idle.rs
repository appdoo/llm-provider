//! Time limits on silence: before a response arrives and between its chunks.

use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use tokio_stream::Stream;

use crate::provider::Error;
use crate::transport::ByteStream;

/// Passes `bytes` through, ending with [`Error::IdleTimeout`] when no item
/// arrives for `gap`.
pub(crate) fn idle_guard<S>(bytes: S, gap: Duration) -> ByteStream
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    todo!()
}

/// Awaits `future`, failing with [`Error::IdleTimeout`] when it takes longer
/// than `gap`.
pub(crate) async fn within<T, E>(
    gap: Duration,
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, Error>
where
    Error: From<E>,
{
    todo!()
}
