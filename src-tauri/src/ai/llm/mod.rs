//! Local LLM subsystem for Phase 3 (Ask Recall).
//!
//! Architecture mirrors `ai/embeddings/`:
//!
//!   * [`AskRecallAdapter`] — trait defining model lifecycle + generation
//!   * [`registry`]         — tier-aware model id + Hugging Face repo metadata
//!   * [`qwen2_candle`]     — candle-based implementation for Qwen2.5 GGUF models
//!
//! v0.4.0 ships tier-aware models (1.5B / 3B / 7B by hardware tier),
//! lazily loaded on first generation, manually unloaded via Settings.
//! Idle reaper + sidecar process isolation are deferred to v0.4.2.
//!
//! No sidecar, no streaming, no multi-turn — those land in v0.4.1+.
//! v0.4.0 is "single-shot Q&A with citations" only.

pub mod qwen2_llama;
pub mod registry;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::app_error::AppResult;

/// Stable identifier for the LLM model in use. Encodes the model name
/// + quantization + tier so we can detect when a tier change requires
/// re-download (similar to how `embedding_model` works on chunk rows).
pub type LlmModelId = &'static str;

/// Generation request — built by the Ask Recall pipeline once it has
/// retrieved + compressed the relevant chunks. Kept narrow on purpose:
/// the adapter doesn't know anything about Recall's data model, just
/// "here's a prompt, give me text back."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmGenerationRequest {
    /// Already-formatted prompt including system + user + chunk
    /// context. The adapter may apply a chat template wrapper on top
    /// (e.g. Qwen's `<|im_start|>` markers) but the prompt-building
    /// caller is responsible for the substantive content.
    pub prompt: String,
    /// Hard cap on generated tokens. Saves us from a runaway model.
    pub max_tokens: usize,
    /// Sampling temperature. 0.0 = greedy. Anything > 0 introduces
    /// randomness; v0.4.0 uses 0.0 for deterministic answers.
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmGenerationResponse {
    pub text: String,
    /// Wall-clock latency from prompt-in to last-token-out.
    pub latency_ms: u64,
    /// Number of tokens generated. Useful for telemetry +
    /// throughput estimates without re-tokenizing.
    pub tokens_generated: u32,
}

#[async_trait]
pub trait AskRecallAdapter: Send + Sync {
    /// Stable model id stored on history rows + reported in Settings.
    fn model_id(&self) -> LlmModelId;

    /// Hugging Face repo this adapter pulls from. Surfaced in
    /// Settings ("downloading from huggingface.co/{repo}") so the
    /// network call is never invisible.
    fn hf_repo(&self) -> &'static str;

    /// True when the model files are present locally and the adapter
    /// is ready to generate without a network call.
    async fn is_ready(&self) -> bool;

    /// Trigger model file download + tokenizer setup if not already
    /// ready. Idempotent: returns quickly when files are already on
    /// disk. Loading the weights into memory happens on first
    /// `generate` call, not here — `prepare` is purely the network
    /// + disk side.
    async fn prepare(&self) -> AppResult<()>;

    /// Run a single-shot generation. The adapter is expected to:
    ///
    ///   1. Lazily load weights into memory on first call (cached
    ///      thereafter).
    ///   2. Apply the chat template appropriate for the model.
    ///   3. Greedy- or temperature-sample up to `max_tokens`.
    ///   4. Stop on the model's natural EOS token.
    ///
    /// v0.4.0 is non-streaming — the future resolves with the full
    /// answer. Streaming graduates to its own trait method in v0.4.1.
    async fn generate(
        &self,
        request: LlmGenerationRequest,
    ) -> AppResult<LlmGenerationResponse>;

    /// v0.4.3: Streaming variant. Calls `on_token` for each token as
    /// it's sampled, returning the same final response shape as
    /// `generate` once generation completes (or hits `max_tokens` /
    /// EOS).
    ///
    /// Default impl falls back to `generate` and emits a single
    /// `on_token` callback with the full text — adapters that
    /// genuinely stream (Qwen2 candle does) override.
    ///
    /// Why streaming matters: candle CPU inference on a 7B model
    /// runs ~2.6 tok/s on Tier C. A 300-token answer takes ~115s.
    /// Without streaming the user stares at a spinner; with
    /// streaming they read along as the answer materializes.
    async fn generate_streaming(
        &self,
        request: LlmGenerationRequest,
        on_token: Box<dyn Fn(String) + Send + Sync>,
    ) -> AppResult<LlmGenerationResponse> {
        // Default impl falls back to the batch path and emits a
        // single callback with the full text. Adapters that
        // genuinely stream (CandleQwen2Adapter does) override.
        // We use owned `String` rather than `&str` for the
        // callback signature to keep the trait HRTB-free —
        // `Box<dyn Fn(&str)>` requires HRTB plumbing that doesn't
        // play nicely with async-trait, and the per-token alloc
        // cost is irrelevant compared to the ~400ms forward pass.
        let response = self.generate(request).await?;
        on_token(response.text.clone());
        Ok(response)
    }

    /// Drop the loaded weights, freeing RAM. Next `generate` call
    /// will lazily reload. v0.4.0 wires this to a manual "Unload
    /// model" button in Settings; the idle reaper that calls this
    /// automatically lands in v0.4.2 alongside the sidecar.
    async fn unload(&self) -> AppResult<()>;
}
