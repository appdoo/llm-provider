//! [`Advisor`]: a tool that hands the calling conversation to another model
//! and returns its advice, and [`CONSULT_ADVISOR`], the prompt paragraph
//! that tells an agent when to call it.

use std::fmt::Display;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;

use crate::agent::macro_support::empty_schema;
use crate::agent::tool::{Tool, ToolOutput, Transcript};
use crate::provider::{Content, Llm, Message, MessageRole, ToolDefinition};

/// When to consult an advisor: a paragraph for the calling agent's system
/// prompt. The caller adds it; nothing adds it automatically.
pub const CONSULT_ADVISOR: &str = "You can consult an advisor tool: a reviewer \
that reads this whole conversation. Call it before you commit to an approach \
and again before you declare the task done. Weigh its advice; do not simply \
obey it. When two advisors disagree, name the disagreement and decide.";

/// The sentence that ends the user message an advisor reads.
const ADVISE: &str = "Advise the agent above: what it should do next and what it is missing.";

/// A [`Tool`] that sends the calling conversation, rendered by
/// [`Transcript::render`], to its own model and returns the answer. The
/// model's input to the tool is ignored.
///
/// The calling agent decides when to call; [`CONSULT_ADVISOR`], put in its
/// system prompt, tells it when. Several advisors with their own
/// [`name`](Self::name) and [`system`](Self::system) persona act as
/// critics whose disagreement stays visible in
/// [`TurnSummary::tool_calls`](crate::agent::TurnSummary::tool_calls).
///
/// An advisor pays off: Anthropic reports Sonnet with an Opus advisor 2.7
/// points higher on SWE-bench Multilingual at 11.9 % lower cost per task,
/// and Haiku with an Opus advisor at 41.2 % on `BrowseComp` against 19.7 %
/// alone. A model criticising its own work does not improve it; a second
/// role with another view does (Huang et al. 2024; Du et al. 2023; Liang et
/// al. 2023).
///
/// ```no_run
/// use llm_provider::agent::{Advisor, ConversationRuntime, Tools, CONSULT_ADVISOR};
/// use llm_provider::Anthropic;
///
/// let advisor = Advisor::new(Anthropic::new("sk-…").llm(Anthropic::CLAUDE_FABLE_5_1)).max_uses(3);
/// let agent = ConversationRuntime::builder(Anthropic::new("sk-…").llm(Anthropic::CLAUDE_OPUS_5_5), Tools::new().with(advisor))
///     .system(CONSULT_ADVISOR)
///     .build();
/// # let _ = agent;
/// ```
#[derive(Debug)]
pub struct Advisor {
    llm: Llm,
    name: String,
    description: String,
    system: Option<String>,
    max_tokens: u32,
    max_uses: Option<usize>,
    uses: Mutex<Uses>,
}

/// The calls of the current agent turn, which the transcript identifies.
#[derive(Debug, Default)]
struct Uses {
    turn: usize,
    count: usize,
}

impl Advisor {
    /// An advisor on `llm` named `advisor`, without a persona, answering
    /// in at most 2048 tokens, with no limit on calls.
    #[must_use]
    pub fn new(llm: Llm) -> Self {
        Self {
            llm,
            name: "advisor".into(),
            description: "A stronger model that reviews your work and names the next step.".into(),
            system: None,
            max_tokens: 2048,
            max_uses: None,
            uses: Mutex::default(),
        }
    }

    /// Sets the tool name the calling model sees (default `advisor`).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the tool description the calling model sees.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Sets the advisor's persona, sent as its system prompt.
    #[must_use]
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets the `max_tokens` of each answer (default 2048).
    #[must_use]
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets how many calls one turn of the calling agent may make; later
    /// calls fail without a request.
    #[must_use]
    pub fn max_uses(mut self, max_uses: usize) -> Self {
        self.max_uses = Some(max_uses);
        self
    }
}

#[async_trait]
impl Tool for Advisor {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: empty_schema(),
        }
    }

    async fn call(&self, _input: serde_json::Value, transcript: &Transcript<'_>) -> ToolOutput {
        if let Some(max) = self.max_uses.filter(|&max| !self.take_use(transcript, max)) {
            return unavailable(format!("limit of {max} calls per turn reached"));
        }

        let prompt = format!("{}\n\n{ADVISE}", transcript.render());
        let mut request = self.llm.request(prompt);
        if let Some(system) = &self.system {
            request = request.system(system);
        }
        match request.max_tokens(self.max_tokens).send().await {
            Ok(answer) => ToolOutput {
                content: answer.text,
                is_error: false,
            },
            Err(error) => unavailable(error),
        }
    }
}

impl Advisor {
    /// Counts one call of the turn `transcript` belongs to; `false` when
    /// the turn already made `max`.
    fn take_use(&self, transcript: &Transcript<'_>, max: usize) -> bool {
        let turn = transcript.messages.iter().filter(|m| is_prompt(m)).count();
        let mut uses = self.uses.lock().unwrap_or_else(PoisonError::into_inner);
        if uses.turn != turn {
            *uses = Uses { turn, count: 0 };
        }
        uses.count += 1;
        uses.count <= max
    }
}

/// Whether `message` is a prompt of the caller, which starts a turn.
fn is_prompt(message: &Message) -> bool {
    message.role == MessageRole::User
        && message
            .content
            .iter()
            .any(|block| matches!(block, Content::Text { .. }))
}

/// A failed call whose text starts with `advisor unavailable: `.
fn unavailable(reason: impl Display) -> ToolOutput {
    ToolOutput {
        content: format!("advisor unavailable: {reason}"),
        is_error: true,
    }
}

#[cfg(test)]
mod tests;
