//! Sending a [`SessionEvent`] for each model call of a
//! [`Session`].

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::Sender;

use crate::agent::error::SessionError;
use crate::agent::event::{EventError, SessionEvent, SessionSetup};
use crate::agent::runtime::{Session, TurnSummary};
use crate::agent::tool::Toolbox;
use crate::provider::{Completion, Error, LlmProvider, Message, Usage};

impl<T: Toolbox> Session<T> {
    /// Sends the event of the model call that began at `started` and ended
    /// with `outcome`, when an event channel is set. `messages` is the
    /// turn's history, reply included; its part from `sent` on is what the
    /// event carries, and `sent` moves to its end. The call was number
    /// `iteration` of its turn. Awaits the `send`; the event gets the next
    /// seq.
    ///
    /// # Errors
    /// [`SessionError::Store`] when the receiver is gone.
    pub(super) async fn emit_call(
        &mut self,
        messages: &[Message],
        sent: &mut usize,
        iteration: usize,
        turn: &TurnSummary,
        started: (SystemTime, Instant),
        outcome: Result<&Completion, &Error>,
    ) -> Result<(), SessionError> {
        let Some(events) = self.events.clone() else {
            return Ok(());
        };

        let added = messages[*sent..].to_vec();
        *sent = messages.len();

        let event = self.model_call(added, iteration, started, outcome);
        self.next_seq += 1;
        self.setup_sent = true;
        send(&events, event, turn).await
    }

    /// Sends the [`SessionEvent::Compacted`] of a compaction that replaced
    /// `before` messages by the session's history, when an event channel is
    /// set.
    ///
    /// # Errors
    /// [`SessionError::Store`] when the receiver is gone.
    pub(in crate::agent) async fn emit_compacted(
        &mut self,
        before: usize,
    ) -> Result<(), SessionError> {
        let Some(events) = self.events.clone() else {
            return Ok(());
        };

        let event = SessionEvent::Compacted {
            session_id: self.id,
            seq: self.next_seq,
            turn: self.turns,
            before,
            after: self.messages.len(),
            history: self.messages.clone(),
        };
        self.next_seq += 1;
        send(&events, event, &TurnSummary::default()).await
    }

    /// Sends the [`SessionEvent::TurnEnded`] of the turn that produced
    /// `result`, when an event channel is set, and returns `result`. A closed
    /// channel turns a successful turn into [`SessionError::Store`]; a turn
    /// that failed keeps its own error.
    pub(in crate::agent) async fn emit_turn_ended(
        &mut self,
        result: Result<TurnSummary, SessionError>,
    ) -> Result<TurnSummary, SessionError> {
        let Some(events) = self.events.clone() else {
            return result;
        };

        let (ok, turn) = match &result {
            Ok(turn) => (true, turn),
            Err(error) => (false, error.turn()),
        };
        let event = SessionEvent::TurnEnded {
            session_id: self.id,
            seq: self.next_seq,
            turn: self.turns,
            ok,
            usage: turn.usage,
        };
        self.next_seq += 1;

        let sent = send(&events, event, turn).await;
        result.and_then(|turn| sent.map(|()| turn))
    }

    /// The event of one model call; its seq is the session's next one.
    fn model_call(
        &self,
        messages: Vec<Message>,
        iteration: usize,
        (started_at, clock): (SystemTime, Instant),
        outcome: Result<&Completion, &Error>,
    ) -> SessionEvent {
        let info = self.llm.info();
        SessionEvent::ModelCall {
            session_id: self.id,
            seq: self.next_seq,
            turn: self.turns,
            iteration,
            model: info.model,
            provider: info.name,
            started_at_ms: millis(started_at.duration_since(UNIX_EPOCH).unwrap_or_default()),
            duration_ms: millis(clock.elapsed()),
            stop_reason: outcome.ok().map(|reply| reply.stop),
            messages,
            usage: outcome.map_or_else(|_| Usage::default(), |reply| reply.usage),
            error: outcome.err().map(EventError::from),
            setup: (!self.setup_sent).then(|| self.setup()),
        }
    }

    /// The system prompt and tool names the session runs with.
    fn setup(&self) -> SessionSetup {
        SessionSetup {
            system: self.settings.system.clone(),
            tools: self
                .toolbox
                .definitions()
                .into_iter()
                .map(|tool| tool.name)
                .collect(),
        }
    }
}

/// `duration` in whole milliseconds, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Sends `event` and awaits room for it.
async fn send(
    events: &Sender<SessionEvent>,
    event: SessionEvent,
    turn: &TurnSummary,
) -> Result<(), SessionError> {
    events
        .send(event)
        .await
        .map_err(|_| SessionError::Store { turn: turn.clone() })
}

#[cfg(test)]
mod tests;
