//! Folding stored [`SessionEvent`]s into the [`Origin`] of a restored
//! session.

use std::collections::HashSet;

use crate::agent::builder::Origin;
use crate::agent::compaction::input_of;
use crate::agent::event::SessionEvent;

impl Origin {
    /// The start of a session restored from `events`; see
    /// [`SessionBuilder::restore`](crate::agent::SessionBuilder::restore) for
    /// the rules.
    pub(in crate::agent) fn from_events(events: impl IntoIterator<Item = SessionEvent>) -> Self {
        let mut events: Vec<SessionEvent> = events.into_iter().collect();
        events.sort_by_key(SessionEvent::seq);
        let Some(last) = events.last() else {
            return Self::fresh();
        };

        // Only the session of the highest-seq event counts.
        let session_id = last.session_id();
        events.retain(|event| event.session_id() == session_id);

        let next_seq = events
            .iter()
            .map(SessionEvent::seq)
            .max()
            .map_or(0, |seq| seq + 1);
        let turns = events.iter().map(SessionEvent::turn).max().unwrap_or(0);

        // The history starts at the last compaction; only turns that ended ok join it.
        let start = events
            .iter()
            .rposition(|event| matches!(event, SessionEvent::Compacted { .. }));
        let mut messages = match start.map(|at| &events[at]) {
            Some(SessionEvent::Compacted { history, .. }) => history.clone(),
            _ => Vec::new(),
        };
        let tail = &events[start.map_or(0, |at| at + 1)..];
        let ok_turns: HashSet<u64> = tail
            .iter()
            .filter_map(|event| match event {
                SessionEvent::TurnEnded { turn, ok: true, .. } => Some(*turn),
                _ => None,
            })
            .collect();
        let mut last_input = 0;
        for event in tail {
            if let SessionEvent::ModelCall {
                turn,
                messages: added,
                usage,
                ..
            } = event
            {
                last_input = input_of(*usage);
                if ok_turns.contains(turn) {
                    messages.extend(added.iter().cloned());
                }
            }
        }

        Self {
            session_id,
            messages,
            next_seq,
            turns,
            last_input,
        }
    }
}

#[cfg(test)]
mod tests;
