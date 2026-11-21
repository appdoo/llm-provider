//! [`CallContext`]: what a tool call receives beside its input — the
//! calling conversation, the turn's cancel token and the turn number.

use tokio_util::sync::CancellationToken;

use crate::agent::tool::Transcript;

/// The context of one tool call: the calling conversation, the turn's
/// cancel token and the turn number.
///
/// A [`ConversationRuntime`](crate::agent::ConversationRuntime) builds one
/// per tool step; outside a runtime turn, [`CallContext::default`] is an
/// empty conversation, a token that never fires and turn `0`.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CallContext<'a> {
    /// The calling agent's conversation.
    pub transcript: Transcript<'a>,
    /// The calling turn's cancel token; one that never fires outside a
    /// runtime turn. A tool waits with `context.cancel.cancelled().await`,
    /// or hands it to work it spawns (a task, a child process): the runtime
    /// drops the tool's own future at once on cancel.
    pub cancel: CancellationToken,
    /// The calling runtime's turn number, counted from 1; `0` outside a
    /// runtime turn. A rolled-back turn keeps its number.
    pub turn: u64,
}

impl CallContext<'_> {
    /// Resolves when the calling turn's [`cancel`](Self::cancel) token
    /// fires; pending forever outside a runtime turn.
    pub async fn cancelled(&self) {
        self.cancel.cancelled().await;
    }
}
