//! Tier-aware model registry for Ask Recall.
//!
//! Maps a [`HardwareTier`] to a concrete LLM choice — Hugging Face
//! repo, GGUF filename, tokenizer filename, and approximate sizes
//! the UI surfaces in download prompts. Centralized here so the
//! adapter, downloader, and Settings tab all agree on what file
//! they're talking about for a given tier.
//!
//! The model picks (Qwen2.5 family, Q4_K_M quantization) reflect the
//! locked Phase 3 plan + the practical perf data on candle CPU
//! inference. We can revisit these without touching anything else
//! that imports the registry.

use crate::ai::hardware::HardwareTier;
use crate::ai::llm::LlmModelId;

#[derive(Debug, Clone, Copy)]
pub struct LlmModelEntry {
    pub model_id: LlmModelId,
    /// Hugging Face repo, like `"Qwen/Qwen2.5-1.5B-Instruct-GGUF"`.
    pub hf_repo: &'static str,
    /// GGUF filename inside the repo. Q4_K_M strikes the best
    /// quality-vs-size balance for our model sizes per the Qwen
    /// team's published benchmarks.
    pub gguf_file: &'static str,
    /// Tokenizer filename — Qwen uses Hugging Face fast-tokenizer
    /// JSON, served from the same repo.
    pub tokenizer_file: &'static str,
    /// Approximate download size for the UI "this is a ~N GB
    /// download" prompt. Real download size may differ slightly
    /// (HF varies by quantization revision); rounded for display.
    pub approx_download_mb: u64,
    /// Approximate RAM during inference. Helps surface "you have
    /// X GB free, this needs Y" warnings in Settings if we ever add
    /// them.
    pub approx_inference_ram_mb: u64,
    /// Hard cap on context tokens — the chat template + chunks +
    /// answer must fit within this. Qwen2.5 supports 32K but we
    /// budget conservatively for memory.
    pub context_window_tokens: usize,
}

/// Pick the model for a given hardware tier.
///
/// | Tier | Model                     | Q4 size | RAM at use |
/// |------|---------------------------|---------|------------|
/// |  A   | Qwen2.5-1.5B-Instruct     |  ~1 GB  |  ~1.5 GB   |
/// |  B   | Qwen2.5-3B-Instruct       |  ~2 GB  |  ~3 GB     |
/// |  C   | Qwen2.5-7B-Instruct       |  ~4 GB  |  ~6 GB     |
pub fn entry_for_tier(tier: HardwareTier) -> LlmModelEntry {
    match tier {
        HardwareTier::A => SMALL,
        HardwareTier::B => MEDIUM,
        HardwareTier::C => LARGE,
    }
}

/// Look up a registry entry by model_id (used when re-instantiating
/// the adapter from a persisted model_id, e.g. after restart).
pub fn entry_by_id(model_id: &str) -> Option<LlmModelEntry> {
    [SMALL, MEDIUM, LARGE]
        .iter()
        .copied()
        .find(|entry| entry.model_id == model_id)
}

const SMALL: LlmModelEntry = LlmModelEntry {
    model_id: "qwen2.5-1.5b-instruct-q4",
    hf_repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
    gguf_file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 1_000,
    approx_inference_ram_mb: 1_500,
    context_window_tokens: 4_096,
};

const MEDIUM: LlmModelEntry = LlmModelEntry {
    model_id: "qwen2.5-3b-instruct-q4",
    hf_repo: "Qwen/Qwen2.5-3B-Instruct-GGUF",
    gguf_file: "qwen2.5-3b-instruct-q4_k_m.gguf",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 2_000,
    approx_inference_ram_mb: 3_000,
    context_window_tokens: 6_144,
};

const LARGE: LlmModelEntry = LlmModelEntry {
    model_id: "qwen2.5-7b-instruct-q4",
    hf_repo: "Qwen/Qwen2.5-7B-Instruct-GGUF",
    gguf_file: "qwen2.5-7b-instruct-q4_k_m.gguf",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 4_400,
    approx_inference_ram_mb: 6_000,
    context_window_tokens: 8_192,
};
