//! A live check of every route and model the crate supports: for each model
//! it asks, streams, asks for a typed answer and runs one tool turn, then
//! prints one Markdown table row.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… AWS_BEARER_TOKEN_BEDROCK=ABSK… \
//!   cargo run --example verify --all-features -- [--route R]… [--model ID]…
//! ```
//!
//! Routes are `anthropic`, `bedrock`, `kimi` and `ollama`. Bedrock runs in
//! `AWS_REGION` (default `eu-central-1`); Ollama answers at `OLLAMA_URL`
//! (default `http://localhost:11434/v1`). A route whose key or server is
//! missing prints its rows as skipped.

use std::convert::Infallible;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use llm_provider::agent::{tool, ConversationRuntime, RuntimeError, Tools};
use llm_provider::{Anthropic, Error, JsonSchema, LlmProvider, OpenAiCompatible};
use serde::Deserialize;

const USAGE: &str = "usage: cargo run --example verify --all-features -- \
[--route anthropic|bedrock|kimi|ollama]… [--model <id>]…";

/// The token limit of every check, with room for a reasoning model's
/// thinking.
const MAX_TOKENS: u32 = 512;

/// The longest reason a failed cell shows.
const REASON_CHARS: usize = 40;

/// One way to reach models: its name on the command line, its label in the
/// table and the models it is checked with.
struct Route {
    name: &'static str,
    label: &'static str,
    models: &'static [&'static str],
}

const ROUTES: &[Route] = &[
    Route {
        name: "anthropic",
        label: "Anthropic API",
        models: &[
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-1",
        ],
    },
    Route {
        name: "bedrock",
        label: "Bedrock /anthropic",
        models: &[
            "eu.anthropic.claude-haiku-5-5",
            "eu.anthropic.claude-sonnet-5-5",
            "eu.anthropic.claude-opus-5-5",
        ],
    },
    Route {
        name: "kimi",
        label: "Bedrock /openai",
        models: &["global.moonshotai.kimi-k3"],
    },
    Route {
        name: "ollama",
        label: "Ollama",
        models: &["llama3.2", "qwen3"],
    },
];

/// Which routes and models to check; empty means all.
#[derive(Default)]
struct Selection {
    routes: Vec<String>,
    models: Vec<String>,
}

/// The answer of the typed check.
#[derive(Deserialize, JsonSchema)]
struct Capital {
    /// The country asked about.
    country: String,
    /// Its capital city.
    city: String,
}

/// Two numbers to add.
#[derive(Deserialize, JsonSchema)]
struct Addends {
    /// The first number.
    a: i64,
    /// The second number.
    b: i64,
}

/// Adds two numbers.
#[tool]
async fn add(addends: Addends) -> Result<String, Infallible> {
    Ok((addends.a + addends.b).to_string())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let selection = selection();
    let started = Instant::now();

    println!("| Route | Model | ask | stream | send_json | tool turn |");
    println!("|---|---|---|---|---|---|");
    for route in ROUTES.iter().filter(|r| selection.wants_route(r.name)) {
        let access = access(route).await;
        for model in route.models.iter().filter(|m| selection.wants_model(m)) {
            let cells = match &access {
                Ok(()) => check_model(&*provider(route, model), model).await,
                Err(why) => skipped(why),
            };
            println!("| {} | {model} | {} |", route.label, cells.join(" | "));
        }
    }

    println!("\n{} · {:.1} s", today(), started.elapsed().as_secs_f64());
}

impl Selection {
    /// Whether `route` is selected.
    fn wants_route(&self, route: &str) -> bool {
        self.routes.is_empty() || self.routes.iter().any(|r| r == route)
    }

    /// Whether `model` is selected.
    fn wants_model(&self, model: &str) -> bool {
        self.models.is_empty() || self.models.iter().any(|m| m == model)
    }
}

/// The selection from the command line; exits 2 with the usage line on an
/// unknown argument or route.
fn selection() -> Selection {
    let mut selection = Selection::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--route", Some(r)) if ROUTES.iter().any(|known| known.name == r) => {
                selection.routes.push(r);
            }
            ("--model", Some(m)) => selection.models.push(m),
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2)
            }
        }
    }
    selection
}

/// `Ok` when the route's key is set and its server answers, else why it is
/// skipped.
async fn access(route: &Route) -> Result<(), String> {
    match route.name {
        "anthropic" => env_set("ANTHROPIC_API_KEY"),
        "bedrock" | "kimi" => env_set("AWS_BEARER_TOKEN_BEDROCK"),
        _ => ollama_reachable().await,
    }
}

/// `Ok` when the variable `name` is set.
fn env_set(name: &str) -> Result<(), String> {
    std::env::var(name)
        .map(drop)
        .map_err(|_| format!("{name} unset"))
}

/// `Ok` when Ollama lists its models within two seconds.
async fn ollama_reachable() -> Result<(), String> {
    let url = format!("{}/models", ollama_url());
    let client = reqwest::Client::new();
    let reply = client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    match reply {
        Ok(r) if r.status().is_success() => Ok(()),
        _ => Err(format!("no server at {}", ollama_url())),
    }
}

/// The provider for `model` on `route`; [`access`] has checked its key.
fn provider(route: &Route, model: &str) -> Box<dyn LlmProvider> {
    let bedrock_key = || env("AWS_BEARER_TOKEN_BEDROCK", "");
    match route.name {
        "anthropic" => Box::new(Anthropic::new(env("ANTHROPIC_API_KEY", ""), model)),
        "bedrock" => Box::new(Anthropic::bedrock(bedrock_key(), region(), model)),
        "kimi" => {
            let url = format!(
                "https://bedrock-runtime.{}.amazonaws.com/openai/v1",
                region()
            );
            let kimi = OpenAiCompatible::builder(url, model).credential(bedrock_key());
            Box::new(kimi.build())
        }
        _ => Box::new(OpenAiCompatible::new(ollama_url(), model)),
    }
}

/// The Bedrock region: `AWS_REGION`, else `eu-central-1`.
fn region() -> String {
    env("AWS_REGION", "eu-central-1")
}

/// Ollama's OpenAI-compatible base URL: `OLLAMA_URL`, else the local default.
fn ollama_url() -> String {
    env("OLLAMA_URL", "http://localhost:11434/v1")
}

/// The variable `name`, or `default` when it is unset.
fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// The four cells of one model, checked one after another.
async fn check_model(provider: &dyn LlmProvider, model: &str) -> Vec<String> {
    vec![
        cell(check_ask(provider).await),
        cell(check_stream(provider).await),
        cell(check_send_json(provider).await),
        cell(check_tool_turn(provider, model).await),
    ]
}

/// The cells of a skipped model: the reason once, then dashes.
fn skipped(why: &str) -> Vec<String> {
    let mut cells = vec!["—".to_owned(); 4];
    cells[0] = format!("skipped: {why}");
    cells
}

/// A check's cell: ✅, or ❌ and its reason cut to [`REASON_CHARS`].
fn cell(result: Result<(), String>) -> String {
    match result {
        Ok(()) => "✅".to_owned(),
        Err(why) => format!("❌ {}", why.chars().take(REASON_CHARS).collect::<String>()),
    }
}

/// One question; passes on non-empty text.
async fn check_ask(provider: &dyn LlmProvider) -> Result<(), String> {
    let request = provider.request("Reply with the word OK.");
    let reply = request
        .max_tokens(MAX_TOKENS)
        .send()
        .await
        .map_err(|e| reason(&e))?;
    non_empty(&reply.text, "empty text")
}

/// One streamed answer; passes on a non-empty delta and non-empty text.
async fn check_stream(provider: &dyn LlmProvider) -> Result<(), String> {
    let request = provider
        .request("Count from 1 to 5.")
        .max_tokens(MAX_TOKENS);
    let mut deltas = request.stream();

    let mut text = String::new();
    while let Some(delta) = deltas.next().await {
        text.push_str(&delta.map_err(|e| reason(&e))?);
    }

    non_empty(&text, "no text deltas")
}

/// One typed answer; passes when its city names Paris.
async fn check_send_json(provider: &dyn LlmProvider) -> Result<(), String> {
    let request = provider.request("What is the capital of France?");
    let capital: Capital = request
        .max_tokens(MAX_TOKENS)
        .send_json()
        .await
        .map_err(|e| reason(&e))?;

    if capital.city.contains("Paris") {
        Ok(())
    } else {
        Err(format!("{}: {}", capital.country, capital.city))
    }
}

/// One agent turn with the `add` tool; passes when it called `add` and
/// answered 42.
async fn check_tool_turn(provider: &dyn LlmProvider, model: &str) -> Result<(), String> {
    let tools = Tools::new().with(Add);
    let mut agent = ConversationRuntime::builder(provider, tools)
        .max_tokens(MAX_TOKENS)
        .build();
    let turn = agent
        .run_turn("What is 17 + 25? Use the add tool.")
        .await
        .map_err(|e| runtime_reason(&e))?;

    if !turn.tool_calls.iter().any(|call| call.name == "add") {
        return Err(format!("{model} called no add"));
    }
    if !turn.response.contains("42") {
        return Err("no 42 in the answer".to_owned());
    }
    Ok(())
}

/// `Ok` when `text` holds more than whitespace, else `why`.
fn non_empty(text: &str, why: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        Err(why.to_owned())
    } else {
        Ok(())
    }
}

/// A short reason for a provider error from its class and HTTP status,
/// never from its message, which may echo a request.
fn reason(error: &Error) -> String {
    match error {
        Error::Api(api) => match api.status() {
            Some(status) => format!("{:?} (HTTP {status})", api.kind()),
            None => format!("{:?}", api.kind()),
        },
        other => format!("{:?}", other.kind()),
    }
}

/// A short reason for a failed agent turn.
fn runtime_reason(error: &RuntimeError) -> String {
    match error {
        RuntimeError::Provider(e) => reason(e),
        RuntimeError::MaxIterations(n) => format!("max iterations ({n})"),
        RuntimeError::Truncated { .. } => "truncated at max_tokens".to_owned(),
        _ => "runtime error".to_owned(),
    }
}

/// Today's date in UTC as `YYYY-MM-DD`.
fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
