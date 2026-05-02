//! v0.4.4 — Ask Recall retrieval primitives.
//!
//! v0.4.3 shipped Ask Recall with a single retrieval path: brute-force
//! cosine over active-model chunks with a hard 0.45 floor. That path
//! has two failure modes the user surfaced immediately:
//!
//!   1. **Opaque-token misses.** "did i save a license key?" embeds
//!      to a query vector that scores ~0.40 against the actual license-
//!      key chunks (centered cosine). The 0.45 floor drops them, and
//!      the `semantic_search` keyword-blend path that would have rescued
//!      them was bypassed entirely. Lesson re-learned: same retrieval
//!      pipeline everywhere, the most-tuned one wins.
//!
//!   2. **Temporal queries.** "summarize my week" doesn't have a
//!      semantic match against any of the user's content — nothing in
//!      their library is *about* "summarizing" or "weeks." Pure cosine
//!      retrieval is the wrong tool. The right tool is a date-range
//!      pull from `memories.created_at`.
//!
//! This module owns the temporal-intent detector. Routing between
//! "use semantic_search" and "pull a date range" lives in the
//! `ask_recall` command itself. Mixed queries ("license keys last
//! week") run both: temporal narrows the candidate set, semantic
//! ranks within it.

pub mod tag_intent;
pub mod temporal;
