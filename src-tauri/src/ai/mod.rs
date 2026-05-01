//! Recall AI subsystem.
//!
//! Phase 1 (v0.2.x) shipped:
//!   * [`hardware`] — tier detection (RAM/CPU/arch/AC state)
//!   * [`ocr`]      — native OCR adapters (Apple Vision on macOS, Windows.Media.Ocr on Windows)
//!   * [`scheduler`] — persisted event-driven work queue
//!
//! Phase 2 (v0.3.0+) adds:
//!   * [`embeddings`] — paragraph-aware chunker, embedding adapter trait,
//!     cosine + MMR similarity for related-memory retrieval. Vectors are
//!     stored on the `memory_chunks` table (one row per chunk) so RAG in
//!     Phase 3 retrieves slices, not whole memories.
//!
//! ## Hard rules (enforced everywhere in this subsystem)
//!
//! 1. AI is **off by default**; enabling/disabling is a settings flip that
//!    drains the queue cleanly without losing data.
//! 2. Save path is never blocked. OCR + embed work is enqueued *after*
//!    the memory commit returns successfully.
//! 3. Idle CPU stays **<1%** with the queue empty. The worker uses
//!    [`tokio::sync::Notify`] wakeups, never timer polling.
//! 4. No cloud calls during normal operation. The embedding model is
//!    downloaded once on opt-in from a pinned URL with sha256 verification.

pub mod embeddings;
pub mod hardware;
pub mod llm;
pub mod ocr;
pub mod scheduler;
