//! v0.5.11 — Ask Recall conversation sessions.
//!
//! Until now Ask Recall was strictly single-shot: each call to
//! `ask_recall` ran retrieval + generation in isolation, threw the
//! result back to the UI, and forgot everything. Multi-turn changes
//! that surface from "ask one question" to "have a conversation
//! grounded in your memories" — and unlocks follow-up questions
//! that build on prior context ("what license keys did I save? ...
//! and which one is for Recall?").
//!
//! v0.5.11 ships in-memory sessions only. State lives on
//! `AppState` for the lifetime of the app process; restarting
//! Recall drops every conversation. Persistence (re-open
//! conversations later) lands in a future release once the
//! storage shape settles.
//!
//! Why the session API instead of just stuffing history into a
//! single command:
//!
//!   * The frontend renders a thread view. Each turn has its own
//!     citations and retrieved sources — preserving them per turn
//!     means the UI can show "the chip you clicked from turn 2
//!     opens the memory turn 2 cited," not just the latest.
//!   * Token-budget management for the LLM prompt needs to know
//!     which turns to keep / drop. Centralized in the session
//!     struct rather than reconstructed from a flat array.
//!   * Cancellation is per-session. A user can have several open
//!     conversations and only cancel the one they're staring at.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde::Serialize;

/// One turn in a conversation. The user message and the assistant
/// reply are separate `Message` rows so the UI can render them as
/// alternating bubbles.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "role")]
pub enum Message {
    User {
        content: String,
        timestamp: String,
    },
    Assistant {
        content: String,
        /// Memories the LLM was given as context for this turn.
        /// Surfaced in the UI as source cards under the message.
        retrieved_sources: Vec<crate::commands::ai::AskRecallCitation>,
        /// Subset of `retrieved_sources` the LLM emitted citation
        /// markers for. Used by the inline numbered chips.
        citations: Vec<crate::commands::ai::AskRecallCitation>,
        tokens_generated: u32,
        latency_ms: u64,
        /// The auto-tag class this turn pivoted on, if any
        /// ("license-key", "url", etc.). `None` for general turns.
        tag_intent: Option<String>,
        timestamp: String,
    },
}

impl Message {
    pub fn role_label(&self) -> &'static str {
        match self {
            Message::User { .. } => "user",
            Message::Assistant { .. } => "assistant",
        }
    }
}

/// One conversation. Lives in memory; lost on app restart.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallSession {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub created_at: String,
}

impl AskRecallSession {
    pub fn new(session_id: String, created_at: String) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            created_at,
        }
    }

    /// Approximate the prompt's token cost from current messages
    /// (chars / 4 heuristic). Used by the prompt builder to decide
    /// when to drop the oldest turn before sending to the LLM.
    pub fn approx_token_count(&self) -> usize {
        let chars: usize = self
            .messages
            .iter()
            .map(|m| match m {
                Message::User { content, .. } => content.len(),
                Message::Assistant {
                    content,
                    retrieved_sources,
                    ..
                } => {
                    content.len()
                        + retrieved_sources
                            .iter()
                            .map(|s| s.chunk_text.len() + 80)
                            .sum::<usize>()
                }
            })
            .sum();
        chars / 4
    }
}

/// Per-session cancellation handle. The generation loop polls
/// the flag every token; flipping it to `true` causes the loop
/// to break on its next iteration and emit a partial response.
///
/// We use AtomicBool rather than a tokio::sync::watch channel
/// because the LLM generation runs inside a `spawn_blocking`
/// closure that can't reliably await — polling an atomic on
/// each token loop iteration is the simplest synchronous check
/// that interops cleanly with both the async caller and the
/// blocking generation loop.
#[derive(Clone, Debug)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Reset the flag for re-use across turns. Each turn starts
    /// fresh; we don't want a previous turn's cancellation to
    /// kill the next one.
    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Default for CancelHandle {
    fn default() -> Self {
        Self::new()
    }
}
