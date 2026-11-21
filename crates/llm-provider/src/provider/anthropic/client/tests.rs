//! Tests for the public client: `ask` and the request builder through the
//! retry policy, the credential headers, a caller's HTTP client and `Debug`.

use std::time::Duration;

use super::Anthropic;
use crate::provider::anthropic::test_support::TEXT_SSE;
use crate::provider::auth::AuthMethod;
use crate::provider::error::{Error, ErrorKind, RequestError};
use crate::provider::llm::Llm;
use crate::provider::retry::RetryPolicy;
use crate::provider::test_stub::{StubReply, StubServer};
use crate::provider::test_support::Summary;
use crate::provider::{CacheTtl, Sampling, ToolDefinition};

async fn front_stub() -> StubServer {
    StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", TEXT_SSE.to_string()),
    )])
    .await
}

fn body_of(stub: &StubServer, i: usize) -> serde_json::Value {
    serde_json::from_str(&stub.requests()[i].body).expect("json body")
}

/// `ask` returns the text and sends key, model, default limit, user message.
#[tokio::test]
async fn ask_sends_key_and_defaults() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");

    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("sk-test")
    );
    let body = body_of(&stub, 0);
    assert_eq!(body["model"], "claude-x");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(
        body["messages"],
        serde_json::json!([{"role":"user","content":[{"type":"text","text":"Hello"}]}])
    );
}

/// a model id constant reaches the wire as its id.
#[tokio::test]
async fn model_constant_sends_its_id() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm(Anthropic::CLAUDE_HAIKU_5_5);

    claude.ask("Hello").await.expect("ask");
    assert_eq!(body_of(&stub, 0)["model"], "claude-haiku-5-5");
}

/// the builder's system, model and limit reach the wire.
#[tokio::test]
async fn request_builder_overrides() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");
    let done = claude
        .request("Hello")
        .system("Be brief.")
        .model("claude-y")
        .max_tokens(256)
        .send()
        .await
        .expect("send");

    assert_eq!(done.text, "Hi");
    let body = body_of(&stub, 0);
    assert_eq!(body["system"], "Be brief.");
    assert_eq!(body["model"], "claude-y");
    assert_eq!(body["max_tokens"], 256);
}

fn overloaded_then_ok() -> Vec<((&'static str, &'static str), Vec<StubReply>)> {
    vec![(
        ("POST", "/v1/messages"),
        vec![
            StubReply::full(
                529,
                "application/json",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
            StubReply::full(200, "text/event-stream", TEXT_SSE),
        ],
    )]
}

/// retry is on by default-shaped policy; a 529 is retried.
#[tokio::test]
async fn retries_overloaded() {
    let stub = StubServer::start_replies(overloaded_then_ok()).await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(
            RetryPolicy::default()
                .max_attempts(4)
                .base(Duration::from_millis(1))
                .cap(Duration::from_millis(10)),
        )
        .build()
        .llm("claude-x");

    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    assert_eq!(stub.requests().len(), 2);
}

/// `RetryPolicy::none()` gives up after the first attempt.
#[tokio::test]
async fn retry_none_gives_up() {
    let stub = StubServer::start_replies(overloaded_then_ok()).await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x");
    let err = claude.ask("Hello").await.expect_err("exhausted");

    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
        "{err:?}"
    );
    assert_eq!(err.kind(), ErrorKind::Overloaded);
    assert_eq!(stub.requests().len(), 1);
}

/// a Bearer credential sends OAuth headers and no `x-api-key`.
#[tokio::test]
async fn bearer_sends_oauth_headers() {
    let stub = front_stub().await;
    let claude = Anthropic::builder(AuthMethod::Bearer("oat-test".into()))
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");

    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    let h = &stub.requests()[0].headers;
    assert_eq!(
        h.get("authorization").map(String::as_str),
        Some("Bearer oat-test")
    );
    assert_eq!(
        h.get("anthropic-beta").map(String::as_str),
        Some("oauth-2025-04-20")
    );
    assert!(!h.contains_key("x-api-key"));
}

/// A caller's HTTP client sends the requests: its user agent reaches the
/// server.
#[tokio::test]
async fn http_client_sends_requests() {
    let stub = front_stub().await;
    let client = reqwest::Client::builder()
        .user_agent("probe")
        .build()
        .expect("client");
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .http_client(client)
        .build()
        .llm("claude-x");

    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");

    let headers = &stub.requests()[0].headers;

    assert_eq!(headers.get("user-agent").map(String::as_str), Some("probe"));
}

/// The default policy makes four attempts; `none()` makes one.
#[test]
fn policy_defaults() {
    assert_eq!(RetryPolicy::default(), RetryPolicy::none().max_attempts(4));
    assert_eq!(RetryPolicy::none(), RetryPolicy::default().max_attempts(1));
}

/// `Debug` shows the origin and never the credential.
#[test]
fn debug_hides_credential() {
    let shown = format!("{:?}", Anthropic::new("sk-secret"));

    assert!(!shown.contains("sk-secret"), "{shown}");
    assert!(shown.contains("\"https://api.anthropic.com\""), "{shown}");
}

/// An event stream whose answer is the text `text`, stopped for `stop`.
fn text_sse(text: &str, stop: &str) -> String {
    let start = serde_json::json!({"type":"content_block_start","index":0,
        "content_block":{"type":"text","text":""}});
    let delta = serde_json::json!({"type":"content_block_delta","index":0,
        "delta":{"type":"text_delta","text":text}});
    let end = serde_json::json!({"type":"message_delta","delta":{"stop_reason":stop},
        "usage":{"output_tokens":5}});
    format!(
        "event: content_block_start\ndata: {start}\n\n\
         event: content_block_delta\ndata: {delta}\n\n\
         event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
         event: message_delta\ndata: {end}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

/// A client against a stub answering every request with `sse`.
async fn json_stub(sse: String) -> (StubServer, Llm) {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", sse),
    )])
    .await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");
    (stub, claude)
}

/// J-1: the answer is parsed from the text; the body asks for structured
/// outputs and forces no tool.
#[tokio::test]
async fn send_as_reads_the_structured_text() {
    let input = r#"{"sentences":["a"],"key_points":["b"]}"#;
    let (stub, claude) = json_stub(text_sse(input, "end_turn")).await;

    let summary: Summary = claude
        .request("x")
        .send_as(Summary::schema())
        .await
        .expect("send_as");

    assert_eq!(summary, Summary::ab());
    let body = body_of(&stub, 0);
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert_eq!(
        body["output_config"]["format"]["schema"]["additionalProperties"],
        false
    );
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("tools").is_none());
}

/// J-3: an answer that does not parse is `InvalidResponse`, naming the
/// field through the answer text.
#[tokio::test]
async fn send_as_rejects_an_answer_of_the_wrong_shape() {
    let (_stub, claude) = json_stub(text_sse(r#"{"sentences":"a"}"#, "end_turn")).await;

    let error = claude
        .request("x")
        .send_as::<Summary>(Summary::schema())
        .await
        .expect_err("bad shape");

    assert_eq!(error.kind(), ErrorKind::InvalidResponse);
    let message = error.to_string();
    assert!(message.contains("sentences"), "{message}");
    assert!(message.contains(r#"{\"sentences\":\"a\"}"#), "{message}");
}

/// J-4: text that is not JSON is `InvalidResponse`.
#[tokio::test]
async fn send_as_rejects_text_that_is_not_json() {
    let (_stub, claude) = json_stub(TEXT_SSE.to_string()).await;

    let error = claude
        .request("x")
        .send_as::<Summary>(Summary::schema())
        .await
        .expect_err("not json");

    assert_eq!(error.kind(), ErrorKind::InvalidResponse);
}

/// J-5: the builder path sends its system prompt with the structured-output
/// format.
#[cfg(feature = "schema")]
#[tokio::test]
async fn send_json_keeps_the_builder_options() {
    let input = r#"{"sentences":["a"],"key_points":["b"]}"#;
    let (stub, claude) = json_stub(text_sse(input, "end_turn")).await;

    let summary = claude
        .request("x")
        .system("s")
        .send_json::<Summary>()
        .await
        .expect("send_json");

    assert_eq!(summary, Summary::ab());
    let body = body_of(&stub, 0);
    assert_eq!(body["system"], "s");
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert!(body.get("tool_choice").is_none());
}

/// A refusal is `InvalidResponse` naming the stop reason, even when its
/// text would parse.
#[tokio::test]
async fn send_as_rejects_a_refusal() {
    let input = r#"{"sentences":["a"],"key_points":["b"]}"#;
    let (_stub, claude) = json_stub(text_sse(input, "refusal")).await;

    let error = claude
        .request("x")
        .send_as::<Summary>(Summary::schema())
        .await
        .expect_err("refusal");

    assert_eq!(error.kind(), ErrorKind::InvalidResponse);
    assert!(error.to_string().contains("refusal"), "{error}");
}

/// An answer cut at the token limit is `InvalidResponse` naming the stop
/// reason.
#[tokio::test]
async fn send_as_rejects_an_answer_cut_at_max_tokens() {
    let (_stub, claude) = json_stub(text_sse(r#"{"sentences":["a"#, "max_tokens")).await;

    let error = claude
        .request("x")
        .send_as::<Summary>(Summary::schema())
        .await
        .expect_err("max_tokens");

    assert_eq!(error.kind(), ErrorKind::InvalidResponse);
    assert!(error.to_string().contains("max_tokens"), "{error}");
}

/// A 429 keeps the provider's `error.type` and `error.message` in the
/// error's text.
#[tokio::test]
async fn rate_limit_error_names_the_provider_message() {
    let body = concat!(
        r#"{"type":"error","error":{"type":"rate_limit_error","#,
        r#""message":"Number of requests has exceeded your rate limit"}}"#
    );
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (429, "application/json", body.to_string()),
    )])
    .await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x");

    let text = claude.ask("Hello").await.expect_err("429").to_string();

    assert!(
        text.contains(
            "API error (RateLimited, 429, rate_limit_error): Number of requests has \
             exceeded your rate limit"
        ),
        "{text}"
    );
}

/// P-1: a model from the Anthropic endpoint posts to `/v1/messages` with
/// the key as `x-api-key` and its id in the body.
#[tokio::test]
async fn llm_posts_its_model_to_the_messages_api() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build();

    claude
        .llm(Anthropic::CLAUDE_HAIKU_5_5)
        .ask("x")
        .await
        .expect("ask");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("sk-test")
    );
    assert_eq!(body_of(&stub, 0)["model"], "claude-haiku-5-5");
}

/// P-8a: batches from the Anthropic endpoint submit to
/// `/v1/messages/batches` with the key and their model.
#[cfg(feature = "batch")]
#[tokio::test]
async fn batches_submit_with_their_model() {
    use std::sync::Arc;

    use crate::provider::batch::{BatchId, BatchProvider, BatchRequest};
    use crate::provider::{CompletionRequest, Content, Message, MessageRole, ModelSelection};

    let answer = r#"{"id":"msgbatch_1","type":"message_batch","processing_status":"in_progress"}"#;
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages/batches"),
        (200, "application/json", answer.to_string()),
    )])
    .await;
    let batches = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .batches(Anthropic::CLAUDE_HAIKU_5_5);
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: "Hi".to_string(),
            }],
        }]),
        tools: vec![],
        max_tokens: 64,
        cache_breakpoints: Vec::new(),
        response_schema: None,
        sampling: crate::provider::Sampling::default(),
    };

    let id = batches
        .submit(vec![BatchRequest::new("a", request).expect("custom_id")])
        .await
        .expect("submit");

    assert_eq!(id, BatchId::from("msgbatch_1".to_string()));
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/messages/batches");
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("sk-test")
    );
    assert_eq!(
        body_of(&stub, 0)["requests"][0]["params"]["model"],
        "claude-haiku-5-5"
    );
}

/// P-15: an OAuth token on the Anthropic API sends `authorization`, the
/// OAuth beta and no `x-api-key`.
#[tokio::test]
async fn oauth_token_sends_the_beta() {
    let stub = front_stub().await;
    let oauth = AuthMethod::Bearer("sk-ant-oat01-x".into());
    let llm = Anthropic::builder(oauth)
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm(Anthropic::CLAUDE_HAIKU_5_5);

    llm.ask("x").await.expect("ask");

    let headers = &stub.requests()[0].headers;
    assert_eq!(
        headers.get("authorization").map(String::as_str),
        Some("Bearer sk-ant-oat01-x")
    );
    assert_eq!(
        headers.get("anthropic-beta").map(String::as_str),
        Some("oauth-2025-04-20")
    );
    assert!(!headers.contains_key("x-api-key"));
}

/// P-21: the default endpoints print their origins.
#[test]
fn default_origins() {
    let anthropic = format!("{:?}", Anthropic::new("k"));
    let openai = format!("{:?}", crate::provider::OpenAi::new("k"));

    assert!(
        anthropic.contains("\"https://api.anthropic.com\""),
        "{anthropic}"
    );
    assert!(openai.contains("\"https://api.openai.com/v1\""), "{openai}");
}

/// An Anthropic model on `stub` that gives up after one attempt.
fn once(stub: &StubServer) -> Llm {
    Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x")
}

/// S1: sampling reaches the body as `temperature`, `top_p`, `stop_sequences`.
#[tokio::test]
async fn sampling_reaches_the_body() {
    let stub = front_stub().await;
    let sampling = Sampling::new().temperature(0.2).top_p(0.9).stop(["X"]);

    once(&stub)
        .request("hi")
        .sampling(sampling)
        .send()
        .await
        .expect("send");

    let body = body_of(&stub, 0);
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["top_p"], 0.9);
    assert_eq!(body["stop_sequences"], serde_json::json!(["X"]));
}

/// S3: a model's 400 about the temperature reaches the caller, not retried.
#[tokio::test]
async fn refused_sampling_surfaces_the_providers_message() {
    let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"temperature is not supported for this model"}}"#;
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![StubReply::full(400, "application/json", body)],
    )])
    .await;

    let err = once(&stub)
        .request("hi")
        .sampling(Sampling::new().temperature(0.2))
        .send()
        .await
        .expect_err("refused");

    assert!(matches!(err, Error::Api(_)), "{err:?}");
    assert_eq!(err.kind(), ErrorKind::InvalidRequest);
    assert!(
        err.to_string().contains("temperature is not supported"),
        "{err}"
    );
    assert_eq!(stub.requests().len(), 1);
}

/// S4: Anthropic has no seed; the request fails before any byte is sent.
#[tokio::test]
async fn seed_is_refused_before_sending() {
    let stub = front_stub().await;

    let err = once(&stub)
        .request("hi")
        .sampling(Sampling::new().seed(7))
        .send()
        .await
        .expect_err("no seed on Anthropic");

    assert_eq!(err.kind(), ErrorKind::InvalidRequest);
    assert_eq!(stub.requests().len(), 0);
}

/// S5: a NaN temperature is `InvalidRequest(NonFinite)`; nothing is sent.
#[tokio::test]
async fn nan_temperature_is_refused_before_sending() {
    let stub = front_stub().await;

    let err = once(&stub)
        .request("hi")
        .sampling(Sampling::new().temperature(f64::NAN))
        .send()
        .await
        .expect_err("nan");

    assert!(
        matches!(
            err,
            Error::InvalidRequest(RequestError::NonFinite("temperature"))
        ),
        "{err:?}"
    );
    assert_eq!(stub.requests().len(), 0);
}

/// An answer "Hi" whose usage wrote 1200 tokens to the cache, 1000 of them
/// with the one-hour TTL.
fn cache_write_sse() -> String {
    let start = serde_json::json!({"type":"message_start","message":{"usage":{
        "input_tokens":3,"cache_creation_input_tokens":1200,"cache_read_input_tokens":0,
        "cache_creation":{"ephemeral_5m_input_tokens":200,"ephemeral_1h_input_tokens":1000}}}});
    let delta = serde_json::json!({"type":"content_block_delta","index":0,
        "delta":{"type":"text_delta","text":"Hi"}});
    let end = serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
        "usage":{"output_tokens":1}});
    format!(
        "event: message_start\ndata: {start}\n\n\
         event: content_block_delta\ndata: {delta}\n\n\
         event: message_delta\ndata: {end}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

/// K1: `cache(OneHour)` marks the tool, the system prompt and the last
/// block of the last message; no top-level `cache_control`.
#[tokio::test]
async fn cache_marks_tools_system_and_last_message_block() {
    let stub = front_stub().await;
    let tool = ToolDefinition {
        name: "t".into(),
        description: "d".into(),
        input_schema: serde_json::json!({"type": "object"}),
    };

    once(&stub)
        .request("hi")
        .system("S")
        .tools([tool])
        .cache(CacheTtl::OneHour)
        .send()
        .await
        .expect("send");

    let body = body_of(&stub, 0);
    let marker = serde_json::json!({"type":"ephemeral","ttl":"1h"});
    assert_eq!(body["tools"][0]["cache_control"], marker);
    assert_eq!(body["system"][0]["cache_control"], marker);
    assert_eq!(body["messages"][0]["content"][0]["cache_control"], marker);
    assert!(body.get("cache_control").is_none(), "{body}");
}

/// K5: the 1h share of the written tokens comes back in the usage.
#[tokio::test]
async fn usage_reports_the_one_hour_share_of_the_cache_write() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", cache_write_sse()),
    )])
    .await;

    let answer = once(&stub).request("hi").send().await.expect("send");

    assert_eq!(answer.usage.cache_creation_input_tokens, 1200);
    assert_eq!(answer.usage.cache_creation_1h_input_tokens, 1000);
}

/// C7: an empty system prompt gets no `cache_control`; the last message
/// block still does.
#[tokio::test]
async fn empty_system_prompt_gets_no_cache_marker() {
    let stub = front_stub().await;

    once(&stub)
        .request("hi")
        .system("")
        .cache(CacheTtl::OneHour)
        .send()
        .await
        .expect("send");

    let body = body_of(&stub, 0);
    let marker = serde_json::json!({"type":"ephemeral","ttl":"1h"});
    assert!(body["system"][0].get("cache_control").is_none(), "{body}");
    assert_eq!(body["messages"][0]["content"][0]["cache_control"], marker);
}

/// C8: `"cache_creation": null` in the usage reads as no split: the call
/// succeeds and the 1h share is 0.
#[tokio::test]
async fn null_cache_creation_reads_as_no_split() {
    let start = serde_json::json!({"type":"message_start","message":{"usage":{
        "input_tokens":3,"cache_creation":null}}});
    let delta = serde_json::json!({"type":"content_block_delta","index":0,
        "delta":{"type":"text_delta","text":"Hi"}});
    let end = serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
        "usage":{"output_tokens":1}});
    let sse = format!(
        "event: message_start\ndata: {start}\n\n\
         event: content_block_delta\ndata: {delta}\n\n\
         event: message_delta\ndata: {end}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    );
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", sse),
    )])
    .await;

    let answer = once(&stub).request("hi").send().await.expect("send");

    assert_eq!(answer.usage.input_tokens, 3);
    assert_eq!(answer.usage.cache_creation_1h_input_tokens, 0);
}
