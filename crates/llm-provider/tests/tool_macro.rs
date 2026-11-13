//! `#[tool]` and `#[tools]` as a caller uses them.
#![cfg(feature = "agent")]

mod stub;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use llm_provider::agent::{tool, tools, Tool, ToolOutput, Toolbox, Tools, Transcript};
use llm_provider::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use stub::Stub;

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, std::io::Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(now.as_secs().to_string())
}

/// Which lines of a text to read.
#[derive(Deserialize, JsonSchema)]
struct LineRange {
    /// The text.
    text: String,
    /// The first line, counting from 0.
    from: usize,
    /// How many lines; all when left out.
    count: Option<usize>,
}

/// Returns some lines of a text.
#[tool]
async fn lines(range: LineRange) -> Result<String, std::convert::Infallible> {
    let lines = range
        .text
        .lines()
        .skip(range.from)
        .take(range.count.unwrap_or(usize::MAX));
    Ok(lines.collect::<Vec<_>>().join("\n"))
}

/// Always fails.
#[tool]
async fn broken() -> Result<String, std::io::Error> {
    Err(std::io::Error::other("disk"))
}

/// Current time, under another name.
#[tool(name = "now")]
async fn renamed_unix_time() -> Result<String, std::io::Error> {
    Ok("0".into())
}

/// M-1: the name and description come from the function; no input means an
/// object schema without required fields.
#[test]
fn definition_of_a_tool_without_input() {
    let definition = UnixTime.definition();

    assert_eq!(definition.name, "unix_time");
    assert_eq!(
        definition.description,
        "Current time in seconds since 1970-01-01 UTC."
    );
    assert_eq!(definition.input_schema["type"], "object");
    assert!(definition.input_schema.get("required").is_none());
}

/// M-2: the schema is the argument struct's, with its field docs.
#[test]
fn definition_schema_comes_from_the_struct() {
    let schema = Lines.definition().input_schema;

    assert_eq!(schema["required"], json!(["text", "from"]));
    assert!(schema["properties"].get("count").is_some(), "{schema}");
    assert_eq!(schema["properties"]["text"]["description"], "The text.");
}

/// M-3: a call parses the input and returns the body's `Ok` text.
#[tokio::test]
async fn call_returns_the_ok_text() {
    let output = Lines
        .call(
            json!({ "text": "a\nb\nc", "from": 1, "count": 1 }),
            &Transcript::default(),
        )
        .await;

    assert_eq!(output, ToolOutput::ok("b"));
}

/// M-4: an input that does not parse is an error naming the field.
#[tokio::test]
async fn call_with_bad_input_names_the_field() {
    let output = Lines
        .call(json!({ "from": 1 }), &Transcript::default())
        .await;

    assert!(output.is_error);
    assert!(
        output.content.starts_with("invalid input: "),
        "{}",
        output.content
    );
    assert!(output.content.contains("text"), "{}", output.content);
}

/// M-5: the body's `Err` is the error text.
#[tokio::test]
async fn call_returns_the_err_text() {
    let output = Broken.call(json!({}), &Transcript::default()).await;

    assert_eq!(output, ToolOutput::error("disk"));
}

/// M-6: `name = "…"` overrides the function name.
#[test]
fn rename_sets_the_tool_name() {
    assert_eq!(RenamedUnixTime.definition().name, "now");
}

/// Two tools over one counter.
struct Counter {
    count: AtomicUsize,
}

#[tools]
impl Counter {
    /// Adds one and returns the count.
    #[tool]
    async fn first(&self) -> Result<usize, std::convert::Infallible> {
        Ok(self.count.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Adds one and returns the count, too.
    #[tool]
    async fn second(&self) -> Result<usize, std::convert::Infallible> {
        Ok(self.count.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Sets the count back to zero; not a tool.
    #[allow(dead_code)]
    fn reset(&self) {
        self.count.store(0, Ordering::SeqCst);
    }
}

/// A counter starting at zero, as a block of tools.
fn counter_tools() -> Tools {
    Tools::new().with_all(Counter {
        count: AtomicUsize::new(0),
    })
}

/// T-1: the tools of a block share one value.
#[tokio::test]
async fn tools_of_a_block_share_state() {
    let tools = counter_tools();

    let first = tools.call("first", json!({}), &Transcript::default()).await;
    let second = tools
        .call("second", json!({}), &Transcript::default())
        .await;

    assert_eq!(first.content, "1");
    assert_eq!(second.content, "2");
}

/// T-2: only `#[tool]` methods become tools, in source order.
#[test]
fn plain_methods_stay_methods() {
    let names: Vec<String> = counter_tools()
        .definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect();

    assert_eq!(names, ["first", "second"]);
}

/// Pages from the web, through one shared client.
struct Web {
    http: reqwest::Client,
}

/// Which page to fetch.
#[derive(Deserialize, JsonSchema)]
struct Page {
    /// The page's URL.
    url: String,
}

#[tools]
impl Web {
    /// Fetches a web page as text.
    #[tool]
    async fn fetch(&self, page: Page) -> Result<String, reqwest::Error> {
        self.http.get(&page.url).send().await?.text().await
    }
}

/// T-5: a block method reaches its state through `&self`.
#[tokio::test]
async fn web_block_fetches_through_its_client() {
    let stub = Stub::start(vec![("text/plain", "hello".into())]).await;
    let tools = Tools::new().with_all(Web {
        http: reqwest::Client::new(),
    });

    let output = tools
        .call(
            "fetch",
            json!({ "url": stub.url() }),
            &Transcript::default(),
        )
        .await;

    assert_eq!(output, ToolOutput::ok("hello"));
}

/// A tool module where another function binds a local named like a tool.
mod shadowing {
    use llm_provider::agent::tool;

    /// Counts to three.
    #[tool]
    pub async fn lines() -> Result<String, std::io::Error> {
        Ok("3".into())
    }

    /// Binds `lines` as a plain variable and squares it.
    pub fn nine() -> i32 {
        let lines = 3;
        lines * lines
    }

    /// An inlined tool.
    #[tool]
    #[inline]
    pub async fn inlined() -> Result<String, std::io::Error> {
        Ok("inline".into())
    }
}

/// `let lines = 3;` beside `#[tool] fn lines` binds a variable.
#[test]
fn a_local_named_like_a_tool_binds() {
    assert_eq!(shadowing::nine(), 9);
}

/// `#[inline]` on a `#[tool]` fn compiles and the tool runs.
#[tokio::test]
async fn an_inline_tool_runs() {
    let output = shadowing::Inlined
        .call(json!({}), &Transcript::default())
        .await;

    assert_eq!(output.content, "inline");
}

/// The function under `#[tool]` stays callable as written.
#[tokio::test]
async fn the_original_function_is_callable() {
    assert_eq!(shadowing::lines().await.unwrap(), "3");
    assert_eq!(
        lines(LineRange {
            text: "a\nb".into(),
            from: 1,
            count: None
        })
        .await
        .unwrap(),
        "b"
    );
}

/// How many messages the calling conversation holds.
#[tool]
async fn message_count(transcript: &Transcript<'_>) -> Result<usize, std::convert::Infallible> {
    Ok(transcript.messages.len())
}

/// Lines of a text, and the length of the calling conversation.
#[tool]
async fn lines_and_count(
    range: LineRange,
    transcript: &Transcript<'_>,
) -> Result<String, std::convert::Infallible> {
    Ok(format!("{} {}", range.from, transcript.messages.len()))
}

/// Reads the calling conversation in a block.
struct Reader;

#[tools]
impl Reader {
    /// The calling agent's system prompt.
    #[tool]
    async fn system(
        &self,
        transcript: &Transcript<'_>,
    ) -> Result<String, std::convert::Infallible> {
        Ok(transcript.system.unwrap_or_default().to_owned())
    }
}

/// X-2: a tool without a transcript parameter behaves as before; one that
/// names `&Transcript<'_>` last sees the calling conversation, and its
/// schema still comes from the input struct alone.
#[tokio::test]
#[allow(
    clippy::field_reassign_with_default,
    reason = "outside the crate Transcript is built from its default, not a struct literal"
)]
async fn transcript_reaches_the_tools_that_name_it() {
    let messages = [llm_provider::Message::user("hi")];
    let mut transcript = Transcript::default();
    transcript.system = Some("S");
    transcript.messages = &messages;

    let plain = Lines
        .call(json!({ "text": "a\nb", "from": 1 }), &transcript)
        .await;
    let counted = MessageCount.call(json!({}), &transcript).await;
    let both = LinesAndCount
        .call(json!({ "text": "a", "from": 2 }), &transcript)
        .await;
    let system = Tools::new()
        .with_all(Reader)
        .call("system", json!({}), &transcript)
        .await;

    assert_eq!(plain.content, "b");
    assert_eq!((counted.content.as_str(), counted.is_error), ("1", false));
    assert_eq!(both.content, "2 1");
    assert_eq!(system.content, "S");
    assert_eq!(
        MessageCount.definition().input_schema,
        json!({ "type": "object", "properties": {} })
    );
    assert_eq!(
        LinesAndCount.definition().input_schema["required"],
        json!(["text", "from"])
    );
}
