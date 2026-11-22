//! [`SessionEvent`]: what a [`Session`](crate::agent::Session) sends the
//! app, over the channel set with
//! [`SessionBuilder::events`](crate::agent::SessionBuilder::events), for
//! every model call it makes, every compaction and every turn end.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{Error, ErrorKind, Message, StopReason, Usage};

/// One thing a session did, in a shape an app stores with one
/// `serde_json::to_string`. Events of a session arrive in
/// [`seq`](Self::seq) order.
///
/// The stored format is a promise: a line written by this version reads back
/// in every later version of the crate. New fields come with a default and
/// no field changes its meaning; `tests/fixtures/session_events.jsonl` pins
/// one line per variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionEvent {
    /// One model call, sent after it ended, whether it succeeded or failed.
    #[non_exhaustive]
    ModelCall {
        /// The session this call belongs to.
        session_id: Uuid,
        /// The event's number in the session: 0 for the first, +1 per event,
        /// never reset by a restore.
        seq: u64,
        /// The turn the call ran in, counted from 1.
        turn: u64,
        /// The call's place in its turn, counted from 1.
        iteration: usize,
        /// The model the call ran on.
        model: String,
        /// The protocol the call spoke, such as `anthropic` or `openai`
        /// ([`ProviderInfo::name`](crate::provider::ProviderInfo::name)): Claude on
        /// Amazon Bedrock is `anthropic`; the host is not recorded.
        provider: String,
        /// When the call started, in milliseconds since the Unix epoch.
        started_at_ms: u64,
        /// How long the call took in milliseconds; its own, not a running
        /// total.
        duration_ms: u64,
        /// Why the model stopped; `None` when the call failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<StopReason>,
        /// The messages this call added to the history: the request's new
        /// messages and the reply, tool calls and tool results in full.
        messages: Vec<Message>,
        /// The tokens of this call alone; price them with
        /// [`Usage::cost`](crate::provider::Usage::cost).
        usage: Usage,
        /// What the call failed with, if it failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<EventError>,
        /// The system prompt and tool names, on the session's first event
        /// only; `None` on every later one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        setup: Option<SessionSetup>,
    },
    /// The history was replaced by the app's compaction callback, sent
    /// before the model call that follows it. A stored session restores to
    /// [`history`](Self::Compacted::history).
    #[non_exhaustive]
    Compacted {
        /// The session that was compacted.
        session_id: Uuid,
        /// The event's number in the session, as for a model call.
        seq: u64,
        /// The turn the compaction ran in, counted from 1.
        turn: u64,
        /// How many messages the history had before.
        before: usize,
        /// How many messages the history has now.
        after: usize,
        /// The new history, in full.
        history: Vec<Message>,
    },
    /// A turn ended, sent after the turn's last model call (or its
    /// compaction), whether the turn succeeded or failed. The events of a
    /// turn with `ok == false` are not part of the history, and a restore
    /// leaves them out. A turn cancelled by its token, or whose future was
    /// dropped, or whose process died, may send no `TurnEnded` at all: a
    /// turn without `TurnEnded` did not join the history.
    #[non_exhaustive]
    TurnEnded {
        /// The session the turn belongs to.
        session_id: Uuid,
        /// The event's number in the session, as for a model call.
        seq: u64,
        /// The turn that ended, counted from 1.
        turn: u64,
        /// Whether the turn succeeded and its messages joined the history.
        ok: bool,
        /// The tokens of the turn's model calls, added up.
        usage: Usage,
    },
}

impl SessionEvent {
    /// The session this event belongs to.
    #[must_use]
    pub fn session_id(&self) -> Uuid {
        match self {
            Self::ModelCall { session_id, .. }
            | Self::Compacted { session_id, .. }
            | Self::TurnEnded { session_id, .. } => *session_id,
        }
    }

    /// The event's number in its session.
    #[must_use]
    pub fn seq(&self) -> u64 {
        match self {
            Self::ModelCall { seq, .. }
            | Self::Compacted { seq, .. }
            | Self::TurnEnded { seq, .. } => *seq,
        }
    }

    /// The turn this event belongs to, counted from 1.
    #[must_use]
    pub fn turn(&self) -> u64 {
        match self {
            Self::ModelCall { turn, .. }
            | Self::Compacted { turn, .. }
            | Self::TurnEnded { turn, .. } => *turn,
        }
    }
}

#[cfg(test)]
mod tests;

/// The system prompt and the tool names a session ran with, so an app can
/// see prompt drift. A restore takes both from the app, not from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionSetup {
    /// The system prompt, if the session has one.
    pub system: Option<String>,
    /// The names of the tools the session offers, in the toolbox's order.
    pub tools: Vec<String>,
}

/// A failed model call as an event stores it: the class of the error and its
/// text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EventError {
    /// The class of the error.
    pub kind: ErrorKind,
    /// The error's message.
    pub message: String,
}

impl From<&Error> for EventError {
    fn from(error: &Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}
