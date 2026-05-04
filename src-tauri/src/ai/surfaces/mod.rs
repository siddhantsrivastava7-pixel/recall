//! Proactive memory surfaces — v0.5.23.
//!
//! Recall's pre-v0.5.23 AI surfaces (Daily recap, Ask Recall) all
//! react to the user — the user opens a memory, asks a question,
//! waits for an answer. The proactive surface engine inverts that:
//! it picks ONE high-confidence card to show at the top of Home so
//! the next time the user opens the app there's already a small
//! moment of "Recall is working for me."
//!
//! Strict product rule for v0.5.23: **at most one card visible at
//! a time.** If both Forgotten Gold and Weekly recap qualify, the
//! engine picks the stronger signal. Stacking surfaces dilutes
//! each one and turns Home into a dashboard — exactly the failure
//! mode this layer is meant to avoid.
//!
//! Selection priority (v0.5.23):
//!   1. **Weekly recap** when today is Monday OR the user hasn't
//!      seen this week's recap yet AND the week has captures.
//!   2. **Forgotten Gold** otherwise, when a strong candidate
//!      exists.
//!   3. **None** when neither qualifies — the slot stays hidden.
//!
//! Each surface kind owns its own picker module:
//!   * `forgotten_gold` — rule-based picker (no embeddings) that
//!     prefers older, well-scored, recently-unopened memories
//!     from projects with current activity.
//!   * `weekly_recap` — composes a per-week recap memory and
//!     surfaces it. LLM summary generated lazily on demand
//!     (same path as Daily recap; reuses `generate_streaming`).
//!
//! Future kinds (v0.5.24+): `project_briefing`, `researched_before`.
//! Both will reuse this module's storage + selection plumbing.

pub mod engine;
pub mod forgotten_gold;
pub mod weekly_recap;

pub use engine::{ActiveSurface, compute_active_surface};
