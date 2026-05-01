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
    /// Hugging Face repo for the GGUF weights file. Qwen's official
    /// GGUF repos use `Qwen/Qwen2.5-{N}-Instruct-GGUF`, but their
    /// 7B Q4_K_M is **sharded into two files** which candle's
    /// `quantized_qwen2::ModelWeights::from_gguf` can't read directly.
    /// For tiers where Qwen ships a single-file Q4_K_M we use their
    /// repo; for the 7B we point at `bartowski/Qwen2.5-7B-Instruct-GGUF`
    /// which packages the same weights as one file.
    pub gguf_repo: &'static str,
    /// GGUF filename inside `gguf_repo`. Q4_K_M strikes the best
    /// quality-vs-size balance per the Qwen team's published
    /// benchmarks. Casing varies by mirror — Qwen uses lowercase,
    /// bartowski capitalizes — so this is stored verbatim.
    pub gguf_file: &'static str,
    /// Hugging Face repo for the tokenizer JSON. Qwen's GGUF repos
    /// don't ship tokenizer.json (only weights), so the tokenizer
    /// has to come from the base instruction-tuned repo,
    /// `Qwen/Qwen2.5-{N}-Instruct`. Splitting these into two repos
    /// is unavoidable given the upstream layout.
    pub tokenizer_repo: &'static str,
    /// Tokenizer filename inside `tokenizer_repo` — always
    /// `tokenizer.json` for Qwen, kept as a field so swapping in
    /// non-Qwen models later is one constant change.
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
    // 1.5B Q4_K_M ships as a single file from Qwen's own repo.
    gguf_repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
    gguf_file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
    tokenizer_repo: "Qwen/Qwen2.5-1.5B-Instruct",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 1_000,
    approx_inference_ram_mb: 1_500,
    context_window_tokens: 4_096,
};

const MEDIUM: LlmModelEntry = LlmModelEntry {
    model_id: "qwen2.5-3b-instruct-q4",
    // 3B Q4_K_M also single-file in Qwen's own repo.
    gguf_repo: "Qwen/Qwen2.5-3B-Instruct-GGUF",
    gguf_file: "qwen2.5-3b-instruct-q4_k_m.gguf",
    tokenizer_repo: "Qwen/Qwen2.5-3B-Instruct",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 2_000,
    approx_inference_ram_mb: 3_000,
    context_window_tokens: 6_144,
};

const LARGE: LlmModelEntry = LlmModelEntry {
    model_id: "qwen2.5-7b-instruct-q4",
    // Qwen's official 7B Q4_K_M is sharded into two files (`-00001-of-00002`
    // and `-00002-of-00002`). candle's `from_gguf` can't load shards
    // directly, so we point at the bartowski mirror which packages the
    // same Q4_K_M quantization as a single file. Functionally identical
    // weights; just packaging differs.
    gguf_repo: "bartowski/Qwen2.5-7B-Instruct-GGUF",
    gguf_file: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
    tokenizer_repo: "Qwen/Qwen2.5-7B-Instruct",
    tokenizer_file: "tokenizer.json",
    approx_download_mb: 4_700,
    approx_inference_ram_mb: 6_000,
    context_window_tokens: 8_192,
};
