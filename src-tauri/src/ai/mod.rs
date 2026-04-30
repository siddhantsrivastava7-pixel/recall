//! Recall AI subsystem.
//!
//! Phase 1 (v0.2.0) ships:
//!   * [`hardware`] — tier detection (RAM/CPU/arch/AC state)
//!   * [`ocr`]      — native OCR adapters (Apple Vision on macOS, Windows.Media.Ocr on Windows)
//!   * [`scheduler`] — persisted event-driven work queue
//!
//! Subsequent phases (v0.3.0+) will add embeddings, semantic search,
//! resurfacing, and an LLM sidecar — those modules do not exist yet.
//!
//! ## Hard rules (enforced everywhere in this subsystem)
//!
//! 1. AI is **off by default**; enabling/disabling is a settings flip that
//!    drains the queue cleanly without losing data.
//! 2. Save path is never blocked. OCR + future embed work is enqueued
//!    *after* the memory commit returns successfully.
//! 3. Idle CPU stays **<1%** with the queue empty. The worker uses
//!    [`tokio::sync::Notify`] wakeups, never timer polling.
//! 4. No cloud calls. No model downloads in Phase 1.

pub mod hardware;
pub mod ocr;
pub mod scheduler;
