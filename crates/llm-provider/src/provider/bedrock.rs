//! [`Bedrock`]: the model ids of Amazon Bedrock.

/// The model ids of Amazon Bedrock. Claude on Bedrock is built with
/// [`Anthropic::bedrock`](crate::Anthropic::bedrock); Kimi K3 goes to
/// Bedrock's OpenAI-compatible route through
/// [`OpenAi`](crate::OpenAi).
///
/// ```no_run
/// use llm_provider::{Anthropic, Bedrock};
///
/// let claude = Anthropic::bedrock("ABSK…", "eu-central-1", Bedrock::EU_CLAUDE_OPUS_5_5);
/// # let _ = claude;
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Bedrock;

impl Bedrock {
    /// Claude Haiku 5.5, the EU cross-region inference profile.
    pub const EU_CLAUDE_HAIKU_5_5: &'static str = "eu.anthropic.claude-haiku-5-5";
    /// Claude Sonnet 5.5, the EU cross-region inference profile.
    pub const EU_CLAUDE_SONNET_5_5: &'static str = "eu.anthropic.claude-sonnet-5-5";
    /// Claude Opus 5.5, the EU cross-region inference profile.
    pub const EU_CLAUDE_OPUS_5_5: &'static str = "eu.anthropic.claude-opus-5-5";
    /// Kimi K3, the global cross-region inference profile on the
    /// OpenAI-compatible route.
    pub const KIMI_K3: &'static str = "global.moonshotai.kimi-k3";
}
