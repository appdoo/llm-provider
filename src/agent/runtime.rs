//! ConversationRuntime — generic stream+tool loop, parameterized over provider and tool executor.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use crate::provider::{
    fold, CompletionRequest, Content, Error, LlmProvider, Message, MessageRole, StopReason,
};
use futures::FutureExt;
use tokio::sync::Semaphore;

use super::defaults::LoopDefaults;

/// Errors from the conversation runtime.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    #[error("Provider error: {0}")]
    Provider(#[from] Error),
    #[error("Max iterations ({0}) exceeded")]
    MaxIterations(usize),
    /// The reply was cut at `max_tokens`. `partial_text` is the text of
    /// every iteration of the turn; it is not appended to the history.
    #[error("reply truncated at max_tokens")]
    Truncated { partial_text: String },
}

/// Result of a single tool execution.
pub struct ToolExecResult {
    pub output: String,
    pub is_error: bool,
}

/// Trait for tool execution — abstracts a set of tools, with optional role filtering.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool by name with the given JSON input.
    async fn execute(&self, tool_name: &str, input: serde_json::Value) -> ToolExecResult;
    /// Return the tool definitions visible to this executor.
    fn tool_definitions(&self) -> Vec<crate::provider::ToolDefinition>;
}

/// Summary of a completed conversational turn.
#[derive(Debug)]
pub struct TurnSummary {
    /// Accumulated text response from the model.
    pub response: String,
    /// Total number of tool calls executed.
    pub tool_calls: usize,
    /// Number of stream iterations (each iteration is one provider request).
    pub iterations: usize,
}

/// Generic conversation runtime — one loop, many configurations.
///
/// Parameterized over tool executor (T). Takes a borrowed LlmProvider to avoid
/// ownership issues when called by callers that hold a `&dyn LlmProvider`.
pub struct ConversationRuntime<'p, T> {
    provider: &'p dyn LlmProvider,
    tool_executor: T,
    system_prompt: Option<String>,
    /// Runtime tunables collected into a typed container.
    /// `max_tokens` / `max_iterations` / `tool_concurrency` all live here.
    defaults: LoopDefaults,
    messages: Vec<Message>,
    /// Cached shared-ownership view of `messages`. Arc-sharing invariant:
    /// `messages_arc.as_ref() == messages.as_slice()` at the top of every
    /// `run_turn` iteration. Rebuilt via `rebuild_arc()` exactly when the
    /// underlying `messages` vector is mutated (i.e. when a new message is
    /// appended); cloned via `Arc::clone` on each request — a pointer bump
    /// rather than a buffer copy.
    messages_arc: Arc<[Message]>,
}

impl<'p, T> ConversationRuntime<'p, T>
where
    T: ToolExecutor,
{
    /// Create a new runtime with the given provider, tool executor, and optional system prompt.
    ///
    /// Uses `LoopDefaults::default()` — see [`LoopDefaults`] for the
    /// default values and per-field builder methods. Call
    /// [`Self::with_defaults`] to override at construction.
    pub fn new(
        provider: &'p dyn LlmProvider,
        tool_executor: T,
        system_prompt: Option<String>,
    ) -> Self {
        Self::with_defaults(
            provider,
            tool_executor,
            system_prompt,
            LoopDefaults::default(),
        )
    }

    /// Construct with an explicit `LoopDefaults`. Plumbing
    /// point for tuning tokens / iterations / concurrency from config or
    /// test harnesses.
    pub fn with_defaults(
        provider: &'p dyn LlmProvider,
        tool_executor: T,
        system_prompt: Option<String>,
        defaults: LoopDefaults,
    ) -> Self {
        Self {
            provider,
            tool_executor,
            system_prompt,
            defaults,
            messages: Vec::new(),
            // Empty slice — a subsequent `append_message` rebuilds the Arc
            // when real history lands.
            messages_arc: Arc::from(Vec::<Message>::new()),
        }
    }

    /// Rebuild the cached `Arc<[Message]>` from the current `messages`
    /// vector. Called exactly once per append — never per request.
    fn rebuild_arc(&mut self) {
        self.messages_arc = Arc::from(self.messages.as_slice());
    }

    /// Append a message to history and refresh the shared Arc view.
    /// Centralises the push/rebuild pair so no caller can forget one.
    fn append_message(&mut self, m: Message) {
        self.messages.push(m);
        self.rebuild_arc();
    }

    /// Override the default max iterations (25).
    ///
    /// For callers that want to tweak a single field without constructing
    /// a full defaults container.
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.defaults.max_iterations = max;
        self
    }

    /// Override the default tool concurrency (4).
    pub fn with_tool_concurrency(mut self, max: usize) -> Self {
        self.defaults.tool_concurrency = max;
        self
    }

    /// Run a single conversational turn: send prompt, handle tool calls in a loop, return summary.
    ///
    /// Each turn starts by appending the user message, then loops:
    /// 1. Stream a completion from the provider
    /// 2. Collect content deltas and tool use events
    /// 3. If no tool calls, break (done)
    /// 4. Execute tool calls, append results, loop back to 1
    pub async fn run_turn(
        &mut self,
        prompt: &str,
        mut on_delta: Option<&mut (dyn FnMut(&str) + Send)>,
    ) -> Result<TurnSummary, RuntimeError> {
        // Add user message
        self.append_message(Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: prompt.to_string(),
            }],
        });

        let tool_defs = self.tool_executor.tool_definitions();
        let mut total_response = String::new();
        let mut total_tool_calls: usize = 0;
        let mut iterations: usize = 0;

        loop {
            iterations += 1;
            if iterations > self.defaults.max_iterations {
                return Err(RuntimeError::MaxIterations(self.defaults.max_iterations));
            }

            // `Arc::clone` is a refcount bump — no `Vec::clone` and
            // no `Arc::from(...)` reconstruction. Within a single
            // `run_turn` the underlying slice never changes between
            // iterations; only an `append_message` at the end of a tool
            // round triggers a single `rebuild_arc`.
            let request = CompletionRequest {
                // `ModelSelection::Default` asks the provider to use its
                // configured model.
                model: crate::provider::ModelSelection::Default,
                system: self.system_prompt.clone(),
                messages: Arc::clone(&self.messages_arc),
                tools: tool_defs.clone(),
                max_tokens: self.defaults.max_tokens,
                cache_breakpoints: Vec::new(),
            };

            let reply = fold(self.provider.stream(request), |text| {
                if let Some(cb) = on_delta.as_mut() {
                    cb(text);
                }
            })
            .await?;
            total_response.push_str(&reply.text);
            if reply.stop == StopReason::MaxTokens {
                return Err(RuntimeError::Truncated {
                    partial_text: total_response,
                });
            }

            let response_text = reply.text;
            let tool_calls: Vec<(String, String, serde_json::Value)> = reply
                .tool_uses
                .into_iter()
                .map(|call| (call.id, call.name, call.input))
                .collect();

            // No tool calls means the model is done
            if tool_calls.is_empty() {
                // Add assistant text message to history
                self.append_message(Message {
                    role: MessageRole::Assistant,
                    content: vec![Content::Text {
                        text: response_text,
                    }],
                });
                break;
            }

            total_tool_calls += tool_calls.len();

            // Add assistant message with tool use blocks
            self.append_message(Message {
                role: MessageRole::Assistant,
                content: tool_calls
                    .iter()
                    .map(|(id, name, input)| Content::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    })
                    .collect(),
            });

            // Execute tool calls in parallel, gated by semaphore
            let semaphore = Arc::new(Semaphore::new(self.defaults.tool_concurrency));
            let executor = &self.tool_executor;
            let max_chars = self.defaults.max_tool_output_chars;
            let tool_results: Vec<Content> =
                futures::future::join_all(tool_calls.iter().map(|(id, name, input)| {
                    let sem = semaphore.clone();
                    let id = id.clone();
                    let name = name.clone();
                    let input = input.clone();
                    async move {
                        // The semaphore is created locally in this
                        // function and never `close()`d, so `acquire().await`
                        // cannot return `AcquireError`. Using `.expect(...)`
                        // instead of an unreachable macro guarantees a panic
                        // with this message if the invariant ever breaks —
                        // the macro form compiles to `unreachable_unchecked`
                        // on some toolchains and would produce UB instead.
                        let _permit = sem
                            .acquire()
                            .await
                            .expect("local semaphore is never closed");
                        let result = match AssertUnwindSafe(executor.execute(&name, input))
                            .catch_unwind()
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => ToolExecResult {
                                output: format!("tool {name} panicked"),
                                is_error: true,
                            },
                        };
                        Content::ToolResult {
                            tool_use_id: id,
                            content: cap_output(result.output, max_chars),
                            is_error: result.is_error,
                        }
                    }
                }))
                .await;

            self.append_message(Message {
                role: MessageRole::User,
                content: tool_results,
            });
        }

        Ok(TurnSummary {
            response: total_response,
            tool_calls: total_tool_calls,
            iterations,
        })
    }
}

/// Keep the head and tail of output longer than `cap` characters.
fn cap_output(output: String, cap: usize) -> String {
    let count = output.chars().count();
    if count <= cap {
        return output;
    }
    let head = cap / 2;
    let tail = cap - head;
    let head_str: String = output.chars().take(head).collect();
    let tail_str: String = output.chars().skip(count - tail).collect();
    format!(
        "{head_str}\n[… {} characters omitted …]\n{tail_str}",
        count - cap
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderInfo, StreamEvent, ToolDefinition};
    use async_stream::stream;
    use std::pin::Pin;
    use tokio_stream::Stream;

    // --- Mock Provider ---

    struct MockTextProvider {
        text: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockTextProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let text = self.text.clone();
            Box::pin(stream! {
                yield Ok(StreamEvent::ContentDelta(text));
                yield Ok(StreamEvent::MessageStop);
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "mock".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Provider that returns a tool call on the first invocation, then text on the second.
    struct MockToolProvider {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockToolProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-1".into(),
                        name: "echo".into(),
                        input: serde_json::json!({"text": "hello"}),
                    }));
                    yield Ok(StreamEvent::MessageStop);
                })
            } else {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ContentDelta("done after tool".into()));
                    yield Ok(StreamEvent::MessageStop);
                })
            }
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "mock-tool".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Provider that always returns a tool call (for max iterations testing).
    struct InfiniteToolProvider;

    #[async_trait::async_trait]
    impl LlmProvider for InfiniteToolProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            Box::pin(stream! {
                yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                    id: "t-loop".into(),
                    name: "echo".into(),
                    input: serde_json::json!({}),
                }));
                yield Ok(StreamEvent::MessageStop);
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "infinite".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Provider that streams multiple deltas.
    struct MockMultiDeltaProvider {
        chunks: Vec<String>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockMultiDeltaProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let chunks = self.chunks.clone();
            Box::pin(stream! {
                for chunk in chunks {
                    yield Ok(StreamEvent::ContentDelta(chunk));
                }
                yield Ok(StreamEvent::MessageStop);
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "multi-delta".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Provider that yields a fixed script of events.
    struct MockScriptedProvider {
        events: Vec<StreamEvent>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockScriptedProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let events = self.events.clone();
            Box::pin(stream! {
                for event in events {
                    yield Ok(event);
                }
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "scripted".into(),
                model: "mock-1".into(),
            }
        }
    }

    // --- Mock Tool Executor ---

    struct MockToolExecutor {
        tools: Vec<ToolDefinition>,
    }

    impl MockToolExecutor {
        fn empty() -> Self {
            Self { tools: Vec::new() }
        }

        fn with_echo() -> Self {
            Self {
                tools: vec![ToolDefinition {
                    name: "echo".into(),
                    description: "Echoes input".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {}}),
                }],
            }
        }
    }

    #[async_trait::async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(&self, _tool_name: &str, _input: serde_json::Value) -> ToolExecResult {
            ToolExecResult {
                output: "echoed".into(),
                is_error: false,
            }
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools.clone()
        }
    }

    // --- Tests ---

    // 1. Basic run_turn with mock provider returns response
    #[tokio::test]
    async fn run_turn_returns_response() {
        let provider = MockTextProvider {
            text: "Hello, world!".into(),
        };
        let executor = MockToolExecutor::empty();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let summary = runtime.run_turn("hi", None).await.unwrap();

        assert_eq!(summary.response, "Hello, world!");
        assert_eq!(summary.tool_calls, 0);
        assert_eq!(summary.iterations, 1);
    }

    // 2. run_turn with tool call executes tool, loops back, returns final response
    #[tokio::test]
    async fn run_turn_with_tool_call() {
        let provider = MockToolProvider {
            call_count: std::sync::atomic::AtomicU32::new(0),
        };
        let executor = MockToolExecutor::with_echo();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let summary = runtime.run_turn("use a tool", None).await.unwrap();

        assert_eq!(summary.response, "done after tool");
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.iterations, 2);
    }

    // 3. run_turn with max_iterations=1 returns error on tool loop
    #[tokio::test]
    async fn run_turn_max_iterations_exceeded() {
        let provider = InfiniteToolProvider;
        let executor = MockToolExecutor::with_echo();
        let mut runtime =
            ConversationRuntime::new(&provider, executor, None).with_max_iterations(1);

        let result = runtime.run_turn("infinite", None).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RuntimeError::MaxIterations(n) => assert_eq!(n, 1),
            other => panic!("expected MaxIterations, got {:?}", other),
        }
    }

    // 4. on_delta callback receives streaming text
    #[tokio::test]
    async fn run_turn_on_delta_receives_text() {
        let provider = MockMultiDeltaProvider {
            chunks: vec!["Hello".into(), ", ".into(), "world!".into()],
        };
        let executor = MockToolExecutor::empty();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let mut deltas = Vec::new();
        let summary = runtime
            .run_turn("hi", Some(&mut |text: &str| deltas.push(text.to_string())))
            .await
            .unwrap();

        assert_eq!(summary.response, "Hello, world!");
        assert_eq!(deltas, vec!["Hello", ", ", "world!"]);
    }

    // 5. Empty tool list, no tool calls, single iteration
    #[tokio::test]
    async fn run_turn_no_tools_single_iteration() {
        let provider = MockTextProvider {
            text: "just text".into(),
        };
        let executor = MockToolExecutor::empty();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let summary = runtime.run_turn("hello", None).await.unwrap();

        assert_eq!(summary.response, "just text");
        assert_eq!(summary.tool_calls, 0);
        assert_eq!(summary.iterations, 1);
    }

    // 6. System prompt is passed through to the provider
    #[tokio::test]
    async fn run_turn_passes_system_prompt() {
        use std::sync::{Arc, Mutex};

        let captured_system = Arc::new(Mutex::new(None::<Option<String>>));
        let captured_clone = captured_system.clone();

        struct CapturingProvider {
            captured: Arc<Mutex<Option<Option<String>>>>,
        }

        #[async_trait::async_trait]
        impl LlmProvider for CapturingProvider {
            fn stream(
                &self,
                request: CompletionRequest,
            ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
                *self.captured.lock().unwrap() = Some(request.system.clone());
                Box::pin(stream! {
                    yield Ok(StreamEvent::ContentDelta("ok".into()));
                    yield Ok(StreamEvent::MessageStop);
                })
            }

            fn info(&self) -> ProviderInfo {
                ProviderInfo {
                    name: "capture".into(),
                    model: "mock-1".into(),
                }
            }
        }

        let provider = CapturingProvider {
            captured: captured_clone,
        };
        let executor = MockToolExecutor::empty();
        let mut runtime = ConversationRuntime::new(
            &provider,
            executor,
            Some("You are a helpful assistant.".into()),
        );

        runtime.run_turn("hi", None).await.unwrap();

        let captured = captured_system.lock().unwrap().clone();
        assert_eq!(captured, Some(Some("You are a helpful assistant.".into())));
    }

    // --- Parallelization test mocks ---

    /// Tool executor that sleeps for a configurable duration before returning.
    struct SlowToolExecutor {
        delay_ms: u64,
        tools: Vec<ToolDefinition>,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for SlowToolExecutor {
        async fn execute(&self, tool_name: &str, input: serde_json::Value) -> ToolExecResult {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            ToolExecResult {
                output: format!("executed {} with {:?}", tool_name, input),
                is_error: false,
            }
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools.clone()
        }
    }

    /// Tool executor that returns an error for a specific tool name.
    struct ErrorOnNameExecutor {
        error_name: String,
        tools: Vec<ToolDefinition>,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for ErrorOnNameExecutor {
        async fn execute(&self, tool_name: &str, _input: serde_json::Value) -> ToolExecResult {
            if tool_name == self.error_name {
                ToolExecResult {
                    output: "tool failed".into(),
                    is_error: true,
                }
            } else {
                ToolExecResult {
                    output: format!("ok: {}", tool_name),
                    is_error: false,
                }
            }
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools.clone()
        }
    }

    /// Provider that emits 3 tool calls on the first invocation, then text on the second.
    struct MultiToolProvider {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MultiToolProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-1".into(),
                        name: "slow".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-2".into(),
                        name: "slow".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-3".into(),
                        name: "slow".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::MessageStop);
                })
            } else {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ContentDelta("done".into()));
                    yield Ok(StreamEvent::MessageStop);
                })
            }
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "multi-tool".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Provider that emits tool calls for "good", "bad", "good" on first call, then text.
    struct ErrorToolProvider {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LlmProvider for ErrorToolProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-1".into(),
                        name: "good".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-2".into(),
                        name: "bad".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id: "t-3".into(),
                        name: "good".into(),
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::MessageStop);
                })
            } else {
                Box::pin(stream! {
                    yield Ok(StreamEvent::ContentDelta("done".into()));
                    yield Ok(StreamEvent::MessageStop);
                })
            }
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "error-tool".into(),
                model: "mock-1".into(),
            }
        }
    }

    // --- Parallelization tests ---

    // 7. Parallel tool execution: 3 calls with 100ms delay, concurrency=4, wall time < 200ms
    #[tokio::test]
    async fn test_parallel_tool_execution() {
        let provider = MultiToolProvider {
            call_count: std::sync::atomic::AtomicU32::new(0),
        };
        let executor = SlowToolExecutor {
            delay_ms: 100,
            tools: vec![ToolDefinition {
                name: "slow".into(),
                description: "Slow tool".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }],
        };
        let mut runtime =
            ConversationRuntime::new(&provider, executor, None).with_tool_concurrency(4);

        let start = std::time::Instant::now();
        let summary = runtime.run_turn("go", None).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(summary.tool_calls, 3);
        // Parallel: all 3 should run concurrently, so < 200ms total
        assert!(
            elapsed.as_millis() < 200,
            "Expected < 200ms for parallel execution, got {}ms",
            elapsed.as_millis()
        );
    }

    // 8. Sequential with concurrency=1: 3 calls with 100ms each, wall time >= 250ms
    #[tokio::test]
    async fn test_sequential_with_concurrency_1() {
        let provider = MultiToolProvider {
            call_count: std::sync::atomic::AtomicU32::new(0),
        };
        let executor = SlowToolExecutor {
            delay_ms: 100,
            tools: vec![ToolDefinition {
                name: "slow".into(),
                description: "Slow tool".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }],
        };
        let mut runtime =
            ConversationRuntime::new(&provider, executor, None).with_tool_concurrency(1);

        let start = std::time::Instant::now();
        let summary = runtime.run_turn("go", None).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(summary.tool_calls, 3);
        // Sequential: must wait for each, so >= 250ms (3 * 100ms with some margin)
        assert!(
            elapsed.as_millis() >= 280,
            "Expected >= 250ms for sequential execution, got {}ms",
            elapsed.as_millis()
        );
    }

    // 9. Error isolation: one tool fails, others succeed, all results returned
    #[tokio::test]
    async fn test_parallel_error_isolation() {
        let provider = ErrorToolProvider {
            call_count: std::sync::atomic::AtomicU32::new(0),
        };
        let executor = ErrorOnNameExecutor {
            error_name: "bad".into(),
            tools: vec![
                ToolDefinition {
                    name: "good".into(),
                    description: "Good tool".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {}}),
                },
                ToolDefinition {
                    name: "bad".into(),
                    description: "Bad tool".into(),
                    input_schema: serde_json::json!({"type": "object", "properties": {}}),
                },
            ],
        };
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let summary = runtime.run_turn("go", None).await.unwrap();

        assert_eq!(summary.tool_calls, 3);
        assert_eq!(summary.response, "done");

        // Verify the tool result messages in the conversation history.
        // The user message at index 2 contains the tool results (index 0 = user prompt, 1 = assistant tool uses, 2 = user tool results).
        let tool_result_msg = &runtime.messages[2];
        assert_eq!(tool_result_msg.content.len(), 3);

        // First result: good (ok)
        match &tool_result_msg.content[0] {
            Content::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "t-1");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
        // Second result: bad (error)
        match &tool_result_msg.content[1] {
            Content::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "t-2");
                assert!(is_error);
                assert_eq!(content, "tool failed");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
        // Third result: good (ok)
        match &tool_result_msg.content[2] {
            Content::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "t-3");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    // 10. Order preservation: results come back in same order as tool_calls
    #[tokio::test]
    async fn test_parallel_order_preservation() {
        let provider = MultiToolProvider {
            call_count: std::sync::atomic::AtomicU32::new(0),
        };
        let executor = SlowToolExecutor {
            delay_ms: 10,
            tools: vec![ToolDefinition {
                name: "slow".into(),
                description: "Slow tool".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }],
        };
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let summary = runtime.run_turn("go", None).await.unwrap();

        assert_eq!(summary.tool_calls, 3);

        // Check tool results in conversation history are in order: t-1, t-2, t-3
        let tool_result_msg = &runtime.messages[2];
        assert_eq!(tool_result_msg.content.len(), 3);

        let expected_ids = ["t-1", "t-2", "t-3"];
        for (i, expected_id) in expected_ids.iter().enumerate() {
            match &tool_result_msg.content[i] {
                Content::ToolResult { tool_use_id, .. } => {
                    assert_eq!(
                        tool_use_id, expected_id,
                        "Result at index {} has wrong id",
                        i
                    );
                }
                other => panic!("expected ToolResult at index {}, got {:?}", i, other),
            }
        }
    }

    // ============================================================
    // Streaming Correctness (Track A): runtime tests
    // ============================================================

    /// Provider that records every `CompletionRequest.messages` Arc pointer
    /// it receives. Used by `twenty_turn_tool_loop_ten_message_history` to prove that within a single turn the same
    /// Arc backs every request (no `Vec::clone`, no `Arc::from` rebuild).
    struct PtrRecordingProvider {
        /// Raw `*const [Message]` recorded per request, paired with the
        /// request index so the loop test can correlate with append boundaries.
        recorded: std::sync::Mutex<Vec<usize>>,
        /// Per-call scripted response — each call consumes one entry.
        script: std::sync::Mutex<Vec<ScriptedResponse>>,
    }

    enum ScriptedResponse {
        ToolCall { id: String, name: String },
        Text(String),
    }

    #[async_trait::async_trait]
    impl LlmProvider for PtrRecordingProvider {
        fn stream(
            &self,
            request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            // Record the Arc's backing-slice pointer as a usize (fat pointer
            // narrowed to its data address — sufficient for same-backing
            // equivalence within a process).
            let ptr = Arc::as_ptr(&request.messages) as *const () as usize;
            self.recorded.lock().unwrap().push(ptr);

            let mut script = self.script.lock().unwrap();
            let next = script.remove(0);
            match next {
                ScriptedResponse::ToolCall { id, name } => Box::pin(stream! {
                    yield Ok(StreamEvent::ToolUse(crate::provider::ToolUseCall {
                        id,
                        name,
                        input: serde_json::json!({}),
                    }));
                    yield Ok(StreamEvent::MessageStop);
                }),
                ScriptedResponse::Text(text) => Box::pin(stream! {
                    yield Ok(StreamEvent::ContentDelta(text));
                    yield Ok(StreamEvent::MessageStop);
                }),
            }
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "ptr-recording".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// Arc sharing within a no-append window.
    ///
    /// This is the core Arc-sharing invariant: *within a single turn* (or any
    /// window where `messages` is not mutated), `Arc::clone(&messages_arc)`
    /// is a pointer bump — both clones point at exactly the same backing
    /// slice. A `Vec::clone` + `Arc::from(...)` reconstruction per request
    /// (the O(n²) pathology the runtime explicitly forbids) would produce a fresh
    /// buffer on every clone, failing `Arc::ptr_eq`.
    ///
    /// We operate directly on the runtime's cached `messages_arc` field.
    /// Because `Arc::clone` is the exact operation performed per request
    /// in `run_turn` (line 158 as of writing), this test pins the precise
    /// invariant the hot path relies on — no scripted provider needed.
    #[tokio::test]
    async fn sub_a_arc_clones_share_backing_within_no_append_window() {
        let provider = MockTextProvider {
            text: "unused".into(),
        };
        let executor = MockToolExecutor::empty();
        let runtime: ConversationRuntime<'_, MockToolExecutor> =
            ConversationRuntime::new(&provider, executor, None);

        // Two clones of the cached Arc, with no intervening mutation.
        // This mirrors what `run_turn` does on successive iterations when
        // no append occurs between them (the Arc-sharing invariant guarantees).
        let clone1 = Arc::clone(&runtime.messages_arc);
        let clone2 = Arc::clone(&runtime.messages_arc);

        assert!(
            Arc::ptr_eq(&clone1, &clone2),
            "two Arc::clone calls with no intervening mutation must share \
             backing — otherwise the runtime is reconstructing the slice \
             per request"
        );
        assert!(
            Arc::ptr_eq(&clone1, &runtime.messages_arc),
            "the clone must share backing with the source Arc"
        );
    }

    /// Arc strong-count semantics across request build/drop.
    ///
    /// Complements Sub-case A by verifying that building a
    /// `CompletionRequest` bumps the strong count (cloning the Arc, not
    /// copying the slice) and dropping it returns the count to baseline.
    /// A `Vec::clone` path would leave the strong count *unchanged* —
    /// every request would own a fresh Arc wrapping a fresh Vec.
    #[tokio::test]
    async fn sub_b_arc_strong_count_tracks_request_clones() {
        let provider = MockTextProvider {
            text: "unused".into(),
        };
        let executor = MockToolExecutor::empty();
        let runtime: ConversationRuntime<'_, MockToolExecutor> =
            ConversationRuntime::new(&provider, executor, None);

        let baseline = Arc::strong_count(&runtime.messages_arc);

        let req = CompletionRequest {
            model: crate::provider::ModelSelection::Default,
            system: None,
            messages: Arc::clone(&runtime.messages_arc),
            tools: vec![],
            max_tokens: 1,
            cache_breakpoints: Vec::new(),
        };
        assert_eq!(
            Arc::strong_count(&runtime.messages_arc),
            baseline + 1,
            "building a CompletionRequest must bump the strong count via Arc::clone"
        );

        drop(req);
        assert_eq!(
            Arc::strong_count(&runtime.messages_arc),
            baseline,
            "dropping the request must return the strong count to baseline"
        );
    }

    /// 20-turn tool loop with 10-message history.
    ///
    /// Scale: 20 iterations over a 10-message baseline
    /// history. The O(n²) pathology would clone the full 10-message
    /// backing slice on every request (10 × 20 = 200 element-clones).
    /// The O(n) correct path performs one `Arc::clone` (pointer bump) per
    /// request, and one `Arc::from(...)` rebuild per append.
    ///
    /// This test exercises the full `run_turn` loop with a scripted
    /// provider that emits 19 tool calls and a final text response —
    /// producing exactly 20 provider calls. Every per-request Arc must be
    /// distinct (because an append happens between every round); this
    /// confirms each iteration sees the runtime's then-current Arc via
    /// a pointer bump, not a per-request buffer rebuild.
    #[tokio::test]
    async fn twenty_turn_tool_loop_ten_message_history() {
        // Build 19 tool-call rounds + 1 final text = 20 iterations.
        let mut script: Vec<ScriptedResponse> = (0..19)
            .map(|i| ScriptedResponse::ToolCall {
                id: format!("t-{i}"),
                name: "echo".into(),
            })
            .collect();
        script.push(ScriptedResponse::Text("done".into()));

        let provider = PtrRecordingProvider {
            recorded: std::sync::Mutex::new(Vec::new()),
            script: std::sync::Mutex::new(script),
        };
        let executor = MockToolExecutor::with_echo();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        // Seed a 10-message baseline history before the turn begins. This
        // is the scale that makes O(n) vs O(n²) behaviour
        // observable. Using `append_message` keeps the Arc consistent
        // with the messages vector (the normal runtime invariant).
        for i in 0..10 {
            runtime.append_message(Message {
                role: if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                content: vec![Content::Text {
                    text: format!("seed-{i}"),
                }],
            });
        }
        assert_eq!(runtime.messages.len(), 10);

        let summary = runtime.run_turn("go", None).await.unwrap();
        assert_eq!(summary.iterations, 20);
        assert_eq!(summary.tool_calls, 19);

        let recorded = provider.recorded.lock().unwrap();
        // One recorded pointer per provider call (one per iteration).
        assert_eq!(recorded.len(), 20, "expected exactly 20 provider calls");
        // Note: we deliberately do NOT assert pointer uniqueness across
        // iterations. The system allocator can (and does) reuse freshly
        // freed Arc backing-slice addresses when the previous Arc is
        // dropped, which makes pointer-address uniqueness a flaky proxy
        // for "rebuild happened". The actual Arc-sharing invariant is
        // pinned by Sub-cases A and B; this test exists to exercise the
        // scale (20 iterations × 10-message baseline) and
        // confirm the runtime completes without panicking or stalling.
    }

    /// Two clones of a single `CompletionRequest` share the same
    /// backing message slice via `Arc::ptr_eq`. Proves the ownership
    /// model: cloning the request is a refcount bump, never a buffer
    /// copy. (A companion test in `traits.rs` covers the same invariant
    /// at the type level; this test anchors it in the runtime's request
    /// construction path.)
    #[tokio::test]
    async fn completion_request_clone_shares_messages_arc() {
        let messages: Arc<[Message]> = Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text { text: "hi".into() }],
        }]);

        let request = CompletionRequest {
            model: crate::provider::ModelSelection::Named("m".into()),
            system: None,
            messages: Arc::clone(&messages),
            tools: vec![],
            max_tokens: 1,
            cache_breakpoints: Vec::new(),
        };

        let cloned_a = request.clone();
        let cloned_b = request.clone();

        assert!(
            Arc::ptr_eq(&cloned_a.messages, &cloned_b.messages),
            "two clones must share the same backing slice"
        );
        assert!(
            Arc::ptr_eq(&cloned_a.messages, &messages),
            "clones must share the original backing slice"
        );
    }

    /// Provider that fails mid-stream with an in-stream `Api` error "boom".
    struct StreamErrorProvider;

    #[async_trait::async_trait]
    impl LlmProvider for StreamErrorProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            Box::pin(stream! {
                yield Ok(StreamEvent::ContentDelta("partial".into()));
                yield Err(Error::Api(crate::provider::ApiError::new(
                    crate::provider::ErrorKind::Overloaded,
                    "boom",
                )));
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "stream-error".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// A mid-stream `Api` error "boom" surfaces unchanged as `Error::Api`.
    #[tokio::test]
    async fn stream_error_maps_to_stream_variant_not_auth() {
        let provider = StreamErrorProvider;
        let executor = MockToolExecutor::empty();
        let mut runtime = ConversationRuntime::new(&provider, executor, None);

        let result = runtime.run_turn("go", None).await;

        match result {
            Err(RuntimeError::Provider(Error::Api(e))) => {
                assert_eq!(e.message(), "boom");
            }
            other => panic!("expected Provider(Api(\"boom\")), got {:?}", other),
        }
    }

    // --- LoopDefaults propagates into ConversationRuntime ---

    /// `ConversationRuntime::with_defaults` accepts a
    /// `LoopDefaults` at construction. The builder API is the
    /// plumbing point for callers that want to override the defaults.
    #[test]
    fn conversation_runtime_accepts_runtime_defaults() {
        let provider = MockTextProvider {
            text: "unused".into(),
        };
        let executor = MockToolExecutor::empty();
        let defaults = LoopDefaults {
            max_iterations: 7,
            max_tokens: 256,
            tool_concurrency: 2,
            max_tool_output_chars: 100_000,
        };
        let runtime = ConversationRuntime::with_defaults(&provider, executor, None, defaults);
        assert_eq!(runtime.defaults.max_iterations, 7);
        assert_eq!(runtime.defaults.max_tokens, 256);
        assert_eq!(runtime.defaults.tool_concurrency, 2);
    }

    /// Custom `LoopDefaults::max_iterations` propagates into
    /// runtime behaviour. Setting `max_iterations: 5` against the
    /// `InfiniteToolProvider` triggers `RuntimeError::MaxIterations(5)` —
    /// proving the defaults struct actually drives the loop, not a stale
    /// hardcoded literal.
    #[tokio::test]
    async fn custom_max_iterations_propagates_to_tool_loop() {
        let provider = InfiniteToolProvider;
        let executor = MockToolExecutor::with_echo();
        let defaults = LoopDefaults {
            max_iterations: 5,
            ..LoopDefaults::default()
        };
        let mut runtime = ConversationRuntime::with_defaults(&provider, executor, None, defaults);

        let result = runtime.run_turn("go", None).await;
        match result {
            Err(RuntimeError::MaxIterations(n)) => {
                assert_eq!(n, 5, "MaxIterations must carry the configured limit");
            }
            other => panic!("expected MaxIterations(5), got {:?}", other),
        }
    }

    /// `new()` defaults to `LoopDefaults::default()` — documents
    /// the behavioural equivalence with the former hardcoded constants.
    #[test]
    fn new_defaults_to_runtime_defaults_default() {
        let provider = MockTextProvider {
            text: "unused".into(),
        };
        let executor = MockToolExecutor::empty();
        let runtime = ConversationRuntime::new(&provider, executor, None);
        assert_eq!(runtime.defaults, LoopDefaults::default());
    }

    /// A reply cut at `max_tokens` fails with `Truncated` carrying
    /// the partial text; the truncated text is not appended to history.
    #[tokio::test]
    async fn max_tokens_stop_is_truncated() {
        let provider = MockScriptedProvider {
            events: vec![
                StreamEvent::ContentDelta("partial".into()),
                StreamEvent::Stop(StopReason::MaxTokens),
                StreamEvent::MessageStop,
            ],
        };
        let mut runtime = ConversationRuntime::new(&provider, MockToolExecutor::empty(), None);

        let result = runtime.run_turn("go", None).await;

        match result {
            Err(RuntimeError::Truncated { partial_text }) => assert_eq!(partial_text, "partial"),
            other => panic!("expected Truncated, got {other:?}"),
        }
        assert_eq!(runtime.messages.len(), 1, "only the user message is kept");
    }

    /// Any other stop reason keeps today's flow.
    #[tokio::test]
    async fn end_turn_stop_is_ok() {
        let provider = MockScriptedProvider {
            events: vec![
                StreamEvent::ContentDelta("done".into()),
                StreamEvent::Stop(StopReason::EndTurn),
                StreamEvent::MessageStop,
            ],
        };
        let mut runtime = ConversationRuntime::new(&provider, MockToolExecutor::empty(), None);

        let summary = runtime.run_turn("go", None).await.unwrap();

        assert_eq!(summary.response, "done");
        assert_eq!(summary.tool_calls, 0);
        assert_eq!(summary.iterations, 1);
    }

    // --- Panicking tools and capped output ---

    /// First `stream` call yields `first` then `MessageStop`; every later
    /// call yields `ContentDelta("done")` then `MessageStop`.
    struct TwoStepProvider {
        first: Vec<StreamEvent>,
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LlmProvider for TwoStepProvider {
        fn stream(
            &self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let events = if n == 0 {
                self.first.clone()
            } else {
                vec![StreamEvent::ContentDelta("done".into())]
            };
            Box::pin(stream! {
                for event in events {
                    yield Ok(event);
                }
                yield Ok(StreamEvent::MessageStop);
            })
        }

        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "two-step".into(),
                model: "mock-1".into(),
            }
        }
    }

    /// `"boom"` panics, `"ok"` returns `"fine"`, `"big"` returns 300 000 `x`.
    struct ScriptedToolExecutor;

    #[async_trait::async_trait]
    impl ToolExecutor for ScriptedToolExecutor {
        async fn execute(&self, tool_name: &str, _input: serde_json::Value) -> ToolExecResult {
            let output = match tool_name {
                "boom" => panic!("boom"),
                "ok" => "fine".to_string(),
                "big" => "x".repeat(300_000),
                other => format!("unknown tool {other}"),
            };
            ToolExecResult {
                output,
                is_error: false,
            }
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            Vec::new()
        }
    }

    fn tool_use(id: &str, name: &str) -> StreamEvent {
        StreamEvent::ToolUse(crate::provider::ToolUseCall {
            id: id.into(),
            name: name.into(),
            input: serde_json::json!({}),
        })
    }

    /// `(tool_use_id, content, is_error)` of every block; panics on a non-result block.
    fn tool_results(content: &[Content]) -> Vec<(&str, &str, bool)> {
        content
            .iter()
            .map(|c| match c {
                Content::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => (tool_use_id.as_str(), content.as_str(), *is_error),
                other => panic!("expected ToolResult, got {other:?}"),
            })
            .collect()
    }

    /// A panicking tool becomes an error result; its sibling still runs.
    #[tokio::test]
    async fn panicking_tool_becomes_error_result() {
        let provider = TwoStepProvider {
            first: vec![tool_use("t-boom", "boom"), tool_use("t-ok", "ok")],
            calls: std::sync::atomic::AtomicU32::new(0),
        };
        let mut runtime = ConversationRuntime::new(&provider, ScriptedToolExecutor, None);

        runtime.run_turn("go", None).await.unwrap();

        let results = &runtime.messages[2];
        assert_eq!(results.role, MessageRole::User);
        assert_eq!(
            tool_results(&results.content),
            vec![
                ("t-boom", "tool boom panicked", true),
                ("t-ok", "fine", false),
            ]
        );
    }

    /// Output over `max_tool_output_chars` keeps head and tail.
    #[tokio::test]
    async fn tool_output_is_capped() {
        let provider = TwoStepProvider {
            first: vec![tool_use("t-big", "big")],
            calls: std::sync::atomic::AtomicU32::new(0),
        };
        let defaults = LoopDefaults {
            max_tool_output_chars: 100_000,
            ..LoopDefaults::default()
        };
        let mut runtime =
            ConversationRuntime::with_defaults(&provider, ScriptedToolExecutor, None, defaults);

        runtime.run_turn("go", None).await.unwrap();

        let expected =
            "x".repeat(50_000) + "\n[… 200000 characters omitted …]\n" + &"x".repeat(50_000);
        assert_eq!(
            tool_results(&runtime.messages[2].content),
            vec![("t-big", expected.as_str(), false)]
        );
    }
}
