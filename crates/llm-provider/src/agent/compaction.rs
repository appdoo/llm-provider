//! Compaction: replacing a long history by the shorter one the app's
//! callback returns (see [`SessionBuilder::compact_when`](crate::agent::SessionBuilder::compact_when)).

use std::error::Error;
use std::future::Future;

use futures::future::BoxFuture;
use futures::FutureExt;

use crate::agent::error::SessionError;
use crate::agent::runtime::{Session, TurnSummary};
use crate::agent::tool::Toolbox;
use crate::provider::{Message, Usage};

/// What a compaction callback fails with: any error, so a callback can use
/// `?` on the errors of its own summarizer.
pub type CompactError = Box<dyn Error + Send + Sync>;

/// The callback as the session stores it.
type Callback =
    Box<dyn FnMut(Vec<Message>) -> BoxFuture<'static, Result<Vec<Message>, CompactError>> + Send>;

/// The compaction settings of a session: when to compact and how.
pub(super) struct Compaction {
    threshold: u64,
    last_input: u64,
    callback: Callback,
}

impl Compaction {
    /// Compaction that runs `callback` once a call's input reaches
    /// `threshold` tokens.
    pub(super) fn new<F, Fut>(threshold: u64, mut callback: F) -> Self
    where
        F: FnMut(Vec<Message>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Vec<Message>, CompactError>> + Send + 'static,
    {
        Self {
            threshold,
            last_input: 0,
            callback: Box::new(move |history| callback(history).boxed()),
        }
    }

    /// Starts the count at `input` tokens, as a restored session knows it.
    pub(super) fn seed(&mut self, input: u64) {
        self.last_input = input;
    }

    /// Whether the last model call's input reached the threshold.
    fn due(&self) -> bool {
        self.last_input >= self.threshold
    }
}

/// The input tokens of a call, cache reads and writes included.
pub(super) fn input_of(usage: Usage) -> u64 {
    u64::from(usage.input_tokens)
        + u64::from(usage.cache_creation_input_tokens)
        + u64::from(usage.cache_read_input_tokens)
}

impl<T: Toolbox> Session<T> {
    /// Remembers the input tokens of the model call that returned `usage`,
    /// cache reads and writes included, when compaction is set.
    pub(super) fn note_input(&mut self, usage: Usage) {
        if let Some(compaction) = &mut self.compaction {
            compaction.last_input = input_of(usage);
        }
    }

    /// At the start of a turn: when the last model call's input reached the
    /// threshold, hands the history to the callback, replaces it with what
    /// it returns at once (a later failure of the turn does not undo it) and
    /// sends a [`SessionEvent::Compacted`](crate::agent::SessionEvent::Compacted).
    ///
    /// # Errors
    /// [`SessionError::Compact`] when the callback fails, with the history
    /// unchanged; [`SessionError::Store`] when the event receiver is gone.
    pub(super) async fn compact_if_due(&mut self) -> Result<(), SessionError> {
        let Some(compaction) = self.compaction.as_mut().filter(|c| c.due()) else {
            return Ok(());
        };

        let before = self.messages.len();
        let compacted = (compaction.callback)(self.messages.clone())
            .await
            .map_err(|error| SessionError::Compact {
                error,
                turn: TurnSummary::default(),
            })?;

        compaction.last_input = 0;
        self.messages = compacted;
        self.emit_compacted(before).await
    }
}
