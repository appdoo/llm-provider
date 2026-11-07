//! Retry with backoff and `Retry-After` for transient provider failures.
//! All re-sending in the crate goes through this module.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_stream::{Stream, StreamExt};

use crate::provider::StreamEvent;

use crate::provider::batch::{BatchId, BatchProvider, BatchRequest, BatchResult, BatchStatus};
use crate::provider::error::{Error, ErrorKind};
use crate::provider::traits::{CompletionRequest, LlmProvider, ProviderInfo};

/// How often and how long to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first; 0 behaves as 1.
    pub max_attempts: u32,
    /// Backoff before the second attempt (before jitter).
    pub base: Duration,
    /// Upper bound for any wait; a larger `Retry-After` gives up.
    pub cap: Duration,
}

impl Default for RetryPolicy {
    /// 4 attempts, base 1 s, cap 60 s.
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(60),
        }
    }
}

/// Waits between attempts. Tests substitute a recorder.
#[async_trait]
pub trait Sleeper: Send + Sync {
    async fn sleep(&self, d: Duration);
}

/// `Sleeper` on `tokio::time::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, d: Duration) {
        tokio::time::sleep(d).await;
    }
}

/// What to do after an attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Not retryable: return the error unchanged.
    Fail,
    /// Retryable, but no attempt or wait is left.
    Exhausted,
    /// Sleep this long, then try again.
    Wait(Duration),
}

/// The decision after attempt `n` (counted from 1) failed with `e`.
pub(crate) fn decide(
    policy: &RetryPolicy,
    n: u32,
    e: &Error,
    retry_if: fn(&Error) -> bool,
) -> Decision {
    if !retry_if(e) {
        return Decision::Fail;
    }
    if n >= policy.max_attempts.max(1) {
        return Decision::Exhausted;
    }
    if let Error::Api(a) = e {
        if let Some(r) = a.retry_after {
            return if r > policy.cap {
                Decision::Exhausted
            } else {
                Decision::Wait(r)
            };
        }
    }
    let factor = 2u32.saturating_pow(n.saturating_sub(1));
    let d = policy.base.saturating_mul(factor).min(policy.cap);
    let half = d / 2;
    let half_nanos = u64::try_from(half.as_nanos()).unwrap_or(u64::MAX);
    Decision::Wait(half + Duration::from_nanos(fastrand::u64(0..=half_nanos)))
}

/// Run `attempt` until it succeeds or the decision says stop.
pub(crate) async fn run<T, F, Fut>(
    policy: &RetryPolicy,
    sleeper: &dyn Sleeper,
    retry_if: fn(&Error) -> bool,
    mut attempt: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let mut n: u32 = 1;
    loop {
        let e = match attempt().await {
            Ok(value) => return Ok(value),
            Err(e) => e,
        };
        match decide(policy, n, &e, retry_if) {
            Decision::Fail => return Err(e),
            Decision::Exhausted => {
                return Err(Error::RetriesExhausted {
                    attempts: n,
                    last: Box::new(e),
                })
            }
            Decision::Wait(t) => {
                sleeper.sleep(t).await;
                n += 1;
            }
        }
    }
}

/// `submit` retries only a request the API rejected, so no batch exists.
fn submit_retry_if(e: &Error) -> bool {
    matches!(
        e,
        Error::Api(a) if a.status.is_some()
            && matches!(a.kind, ErrorKind::RateLimited | ErrorKind::Overloaded | ErrorKind::Server)
    )
}

/// Wraps a provider and retries transient failures per `RetryPolicy`.
pub struct RetryingProvider<P> {
    inner: Arc<P>,
    policy: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
}

impl<P> RetryingProvider<P> {
    /// Wrap `inner`, waiting with `TokioSleeper`.
    pub fn new(inner: P, policy: RetryPolicy) -> Self {
        Self::with_sleeper(inner, policy, Arc::new(TokioSleeper))
    }

    /// Wrap `inner` with an explicit `Sleeper`.
    pub fn with_sleeper(inner: P, policy: RetryPolicy, sleeper: Arc<dyn Sleeper>) -> Self {
        Self {
            inner: Arc::new(inner),
            policy,
            sleeper,
        }
    }
}

impl<P> fmt::Debug for RetryingProvider<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryingProvider")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<P: LlmProvider + 'static> LlmProvider for RetryingProvider<P> {
    /// Retries while only `Usage` has arrived (the retry window); after the
    /// first content event every item is forwarded and never retried.
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
        let inner = Arc::clone(&self.inner);
        let policy = self.policy;
        let sleeper = Arc::clone(&self.sleeper);
        Box::pin(async_stream::stream! {
            let mut n: u32 = 1;
            loop {
                let mut events = inner.stream(request.clone());
                let mut buffer: Vec<StreamEvent> = Vec::new();
                // Retry window.
                let first = loop {
                    match events.next().await {
                        Some(Ok(usage @ StreamEvent::Usage { .. })) => buffer.push(usage),
                        other => break other,
                    }
                };
                match first {
                    None => {
                        for ev in buffer {
                            yield Ok(ev);
                        }
                        return;
                    }
                    Some(Ok(ev)) => {
                        for buffered in buffer {
                            yield Ok(buffered);
                        }
                        yield Ok(ev);
                    }
                    Some(Err(e)) => match decide(&policy, n, &e, Error::is_retryable) {
                        Decision::Fail => {
                            for ev in buffer {
                                yield Ok(ev);
                            }
                            yield Err(e);
                            return;
                        }
                        Decision::Exhausted => {
                            yield Err(Error::RetriesExhausted { attempts: n, last: Box::new(e) });
                            return;
                        }
                        Decision::Wait(t) => {
                            drop(buffer);
                            sleeper.sleep(t).await;
                            n += 1;
                            continue;
                        }
                    },
                }
                // Forwarding.
                while let Some(item) = events.next().await {
                    yield item;
                }
                return;
            }
        })
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
    }
}

#[async_trait]
impl<P: BatchProvider + 'static> BatchProvider for RetryingProvider<P> {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        run(&self.policy, &*self.sleeper, submit_retry_if, || {
            self.inner.submit(requests.clone())
        })
        .await
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        run(&self.policy, &*self.sleeper, Error::is_retryable, || {
            self.inner.status(id)
        })
        .await
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        run(&self.policy, &*self.sleeper, Error::is_retryable, || {
            self.inner.results(id)
        })
        .await
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.cancel(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::anthropic::AnthropicApi;
    use crate::provider::auth::AuthMethod;
    use crate::provider::error::ApiError;
    use crate::provider::test_stub::{RecordingSleeper, StubReply, StubServer};
    use crate::provider::traits::{Content, Message, MessageRole, ModelSelection};

    const TEXT_SSE: &str = include_str!("../../tests/fixtures/anthropic_text.sse");
    const RATE_LIMIT: &str =
        r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
    const OVERLOADED: &str =
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
    const INVALID: &str =
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
    const MESSAGES: (&str, &str) = ("POST", "/v1/messages");

    fn json(status: u16, body: &str) -> StubReply {
        StubReply::full(status, "application/json", body)
    }

    fn sse(body: &str) -> StubReply {
        StubReply::full(200, "text/event-stream", body)
    }

    fn with_headers(mut reply: StubReply, headers: &[(&'static str, &str)]) -> StubReply {
        reply.headers = headers.iter().map(|(k, v)| (*k, v.to_string())).collect();
        reply
    }

    fn in_stream_overloaded() -> String {
        format!("event: error\ndata: {OVERLOADED}\n\n")
    }

    async fn setup(
        replies: Vec<StubReply>,
        policy: RetryPolicy,
    ) -> (
        StubServer,
        RetryingProvider<AnthropicApi>,
        Arc<RecordingSleeper>,
    ) {
        let stub = StubServer::start_replies(vec![(MESSAGES, replies)]).await;
        let recorder = Arc::new(RecordingSleeper::default());
        let provider = RetryingProvider::with_sleeper(
            AnthropicApi::with_base_url(
                "m",
                AuthMethod::ApiKey("test-key".to_string()),
                &stub.url(),
            ),
            policy,
            recorder.clone(),
        );
        (stub, provider, recorder)
    }

    fn request() -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "hello".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: Vec::new(),
        }
    }

    fn api_of(e: &Error) -> &ApiError {
        match e {
            Error::Api(a) => a,
            other => panic!("expected Api, got {other:?}"),
        }
    }

    /// 429 with `retry-after: 2`, then 200: one sleep of exactly 2 s.
    #[tokio::test]
    async fn retry_after_is_honoured() {
        let first = with_headers(json(429, RATE_LIMIT), &[("retry-after", "2")]);
        let (stub, provider, recorder) =
            setup(vec![first, sse(TEXT_SSE)], RetryPolicy::default()).await;
        let completion = provider.complete(request()).await.expect("complete");
        assert_eq!(completion.text, "Hi");
        assert_eq!(stub.requests().len(), 2);
        assert_eq!(recorder.slept(), vec![Duration::from_secs(2)]);
    }

    /// 529 then 200: one jittered sleep in 0.5 s..=1 s.
    #[tokio::test]
    async fn backoff_with_jitter() {
        let (stub, provider, recorder) = setup(
            vec![json(529, OVERLOADED), sse(TEXT_SSE)],
            RetryPolicy::default(),
        )
        .await;
        provider.complete(request()).await.expect("complete");
        assert_eq!(stub.requests().len(), 2);
        let slept = recorder.slept();
        assert_eq!(slept.len(), 1);
        assert!(
            slept[0] >= Duration::from_millis(500) && slept[0] <= Duration::from_secs(1),
            "sleep out of range: {:?}",
            slept[0]
        );
    }

    /// 400 invalid request is not retried.
    #[tokio::test]
    async fn invalid_request_not_retried() {
        let (stub, provider, recorder) =
            setup(vec![json(400, INVALID)], RetryPolicy::default()).await;
        let err = provider.complete(request()).await.expect_err("must fail");
        assert_eq!(api_of(&err).kind, ErrorKind::InvalidRequest);
        assert_eq!(stub.requests().len(), 1);
        assert!(recorder.slept().is_empty());
    }

    /// 401 is not retried.
    #[tokio::test]
    async fn auth_not_retried() {
        let (stub, provider, recorder) = setup(
            vec![StubReply::full(401, "text/plain", "no")],
            RetryPolicy::default(),
        )
        .await;
        let err = provider.complete(request()).await.expect_err("must fail");
        assert!(matches!(err, Error::Api(_)), "got {err:?}");
        assert_eq!(err.kind(), ErrorKind::Auth);
        assert_eq!(stub.requests().len(), 1);
        assert!(recorder.slept().is_empty());
    }

    /// 529 on every request: 4 attempts, 3 sleeps, then exhausted.
    #[tokio::test]
    async fn exhausted_after_four_attempts() {
        let (stub, provider, recorder) =
            setup(vec![json(529, OVERLOADED)], RetryPolicy::default()).await;
        let err = provider.complete(request()).await.expect_err("must fail");
        assert!(
            matches!(err, Error::RetriesExhausted { attempts: 4, .. }),
            "got {err:?}"
        );
        assert_eq!(err.kind(), ErrorKind::Overloaded);
        assert_eq!(stub.requests().len(), 4);
        assert_eq!(recorder.slept().len(), 3);
    }

    /// `retry-after` beyond the cap gives up at once.
    #[tokio::test]
    async fn retry_after_over_cap_gives_up() {
        let first = with_headers(json(429, RATE_LIMIT), &[("retry-after", "120")]);
        let (stub, provider, recorder) = setup(vec![first], RetryPolicy::default()).await;
        let err = provider.complete(request()).await.expect_err("must fail");
        assert!(
            matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
            "got {err:?}"
        );
        assert_eq!(stub.requests().len(), 1);
        assert!(recorder.slept().is_empty());
    }

    /// The cap also applies on a later attempt.
    #[test]
    fn cap_on_later_attempt() {
        let e = Error::Api(ApiError {
            kind: ErrorKind::RateLimited,
            status: Some(429),
            message: "slow down".to_string(),
            request_id: None,
            retry_after: Some(Duration::from_secs(120)),
        });
        assert_eq!(
            decide(&RetryPolicy::default(), 2, &e, Error::is_retryable),
            Decision::Exhausted
        );
    }

    /// An error after only `message_start` is retried; usage of the
    /// failed attempt is dropped, so input tokens count once.
    #[tokio::test]
    async fn retry_window_drops_buffered_usage() {
        let start = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n";
        let first = sse(&format!("{start}{}", in_stream_overloaded()));
        let (stub, provider, _recorder) =
            setup(vec![first, sse(TEXT_SSE)], RetryPolicy::default()).await;
        let completion = provider.complete(request()).await.expect("complete");
        assert_eq!(stub.requests().len(), 2);
        assert_eq!(completion.usage.input_tokens, 10);
    }

    /// After content arrived, an error is forwarded and never retried.
    #[tokio::test]
    async fn no_retry_after_content() {
        let delta = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
        let body = format!("{delta}{}", in_stream_overloaded());
        let (stub, provider, _recorder) = setup(vec![sse(&body)], RetryPolicy::default()).await;
        let items: Vec<_> = provider.stream(request()).collect().await;
        assert_eq!(items.len(), 2, "items: {items:?}");
        assert!(
            matches!(&items[0], Ok(StreamEvent::ContentDelta(t)) if t == "Hi"),
            "first: {:?}",
            items[0]
        );
        match &items[1] {
            Err(e) => {
                let a = api_of(e);
                assert_eq!(a.kind, ErrorKind::Overloaded);
                assert_eq!(a.status, None);
            }
            other => panic!("expected Err, got {other:?}"),
        }
        assert_eq!(stub.requests().len(), 1);
    }

    /// `max_attempts: 1` turns retry off.
    #[tokio::test]
    async fn one_attempt_is_retry_off() {
        let policy = RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        };
        let (stub, provider, _recorder) =
            setup(vec![json(529, OVERLOADED), sse(TEXT_SSE)], policy).await;
        let err = provider.complete(request()).await.expect_err("must fail");
        assert!(
            matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
            "got {err:?}"
        );
        assert_eq!(stub.requests().len(), 1);
    }

    /// The request id survives into `RetriesExhausted.last`.
    #[tokio::test]
    async fn request_id_end_to_end() {
        let policy = RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        };
        let reply = with_headers(
            json(429, RATE_LIMIT),
            &[("request-id", "req_123"), ("retry-after", "2")],
        );
        let (_stub, provider, _recorder) = setup(vec![reply], policy).await;
        match provider.complete(request()).await {
            Err(Error::RetriesExhausted { last, .. }) => {
                assert_eq!(api_of(&last).request_id.as_deref(), Some("req_123"));
            }
            other => panic!("expected RetriesExhausted, got {other:?}"),
        }
    }
}
