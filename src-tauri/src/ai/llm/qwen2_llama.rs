//! llama.cpp-based adapter for Qwen2.5-Instruct GGUF models.
//!
//! v0.5.0: replaces the v0.4.x candle adapter. Same `AskRecallAdapter`
//! contract, same download path, same chat template, same streaming
//! callback shape — only the inference engine changes. The candle
//! version's prefill speed (~2.6 tok/s same as generation) made
//! 1000-token RAG prompts take ~6 minutes before the first token.
//! llama.cpp's hand-tuned SIMD prefill drops that to 7–20s on the
//! same hardware.
//!
//! What this means in practice for the rest of the codebase:
//!   * The `qwen2_candle` module name is gone. Importers update to
//!     `qwen2_llama`.
//!   * The separate `tokenizer.json` download is no longer required
//!     — llama.cpp reads the tokenizer embedded in the GGUF. Existing
//!     installs have an orphan tokenizer.json on disk; harmless.
//!   * The `LlmDownloadProgress` event payload + `recall://llm-
//!     download-progress` channel are unchanged so the Settings UI
//!     keeps working without a frontend change.
//!
//! Threading: llama.cpp's `decode()` is sync + CPU-bound. We run it
//! on a `tokio::task::spawn_blocking` thread so the runtime stays
//! responsive during a long prefill or generation. Unlike candle,
//! the llama.cpp `LlamaContext` *is* `Send + Sync` once moved out of
//! the Mutex, so we can ship it across the spawn_blocking boundary
//! without contortion.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::ai::llm::registry::LlmModelEntry;
use crate::ai::llm::{
    AskRecallAdapter, LlmGenerationRequest, LlmGenerationResponse, LlmModelId,
};
use crate::errors::app_error::{AppError, AppResult};

/// Subdirectory under `app_data_dir()` where the LLM model files
/// live. Distinct from the embedding model dir so a future model
/// GC pass can target one without affecting the other.
const CACHE_SUBDIR: &str = "models/llm";

/// Context window in tokens. Qwen2.5-Instruct supports up to 32K
/// natively. v0.5.0 set this at 4K to keep KV cache RAM small, but
/// real RAG queries tokenized denser than the char/4 estimate
/// (license keys, code, structured text) and overflowed at ~4300
/// tokens. v0.5.1 bumps to 8K — Qwen2.5 7B GQA-4 KV cache costs
/// ~57 KB/token so 8K = ~470 MB resident, irrelevant on Tier C
/// (32GB+). Tier A (8GB) might want a smaller cap; revisit if a
/// real user reports memory pressure.
const CONTEXT_WINDOW_TOKENS: u32 = 8_192;

/// Event payload for `recall://llm-download-progress`. Unchanged
/// from v0.4.x — the Settings UI listens on this channel and
/// renders a real progress bar from the `(bytes_downloaded,
/// bytes_total)` pair.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmDownloadProgress {
    pub phase: &'static str,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub message: String,
}

pub struct LlamaQwen2Adapter {
    app: AppHandle,
    entry: LlmModelEntry,
    /// llama.cpp's global backend. `LlamaBackend::init` initializes
    /// the global llama_init_backend() exactly once per process,
    /// later calls return an error. We hold an `Arc` so the model
    /// + context can borrow it for their lifetimes.
    backend: Arc<LlamaBackend>,
    /// Loaded model + context. None when unloaded; `generate`
    /// populates on first call. The mutex serializes generation —
    /// one call at a time on the shared context, which is fine for
    /// our single-flight Ask Recall surface.
    state: Mutex<Option<LoadedModel>>,
}

struct LoadedModel {
    model: LlamaModel,
}

impl LlamaQwen2Adapter {
    pub fn new(app: AppHandle, entry: LlmModelEntry) -> AppResult<Self> {
        // Backend init happens at construction so a missing backend
        // (e.g. unsupported CPU) fails the adapter build, not the
        // first generate call. Initializing twice in the same
        // process returns an error which we surface as a panic
        // because that means our own lifecycle is broken.
        let backend = LlamaBackend::init().map_err(|err| {
            AppError::Invalid(format!("llama.cpp backend init failed: {err}"))
        })?;
        Ok(Self {
            app,
            entry,
            backend: Arc::new(backend),
            state: Mutex::new(None),
        })
    }

    fn cache_dir(&self) -> AppResult<PathBuf> {
        let base = self
            .app
            .path()
            .app_data_dir()
            .map_err(|err| AppError::Invalid(format!("app_data_dir unavailable: {err}")))?;
        let dir = base.join(CACHE_SUBDIR);
        std::fs::create_dir_all(&dir).map_err(|err| {
            AppError::Invalid(format!(
                "Failed to create LLM cache {}: {err}",
                dir.display()
            ))
        })?;
        Ok(dir)
    }

    fn gguf_path(&self) -> AppResult<PathBuf> {
        Ok(self.cache_dir()?.join(self.entry.gguf_file))
    }

    /// Pull GGUF from its HF repo into the cache dir. v0.5.0 drops
    /// the separate tokenizer.json download — llama.cpp parses the
    /// tokenizer out of the GGUF directly. Existing v0.4.x installs
    /// have an orphan tokenizer.json on disk; harmless, ignored.
    ///
    /// Idempotent — files already on disk are skipped (and emit a
    /// `phase = "complete"` final event so the UI can flip cleanly).
    async fn ensure_files_downloaded(&self) -> AppResult<()> {
        let gguf_path = self.gguf_path()?;
        if gguf_path.exists() {
            self.emit_progress(LlmDownloadProgress {
                phase: "complete",
                bytes_downloaded: 0,
                bytes_total: 0,
                message: "Model already on disk.".into(),
            });
            return Ok(());
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.entry.gguf_repo, self.entry.gguf_file
        );
        self.stream_download(&url, &gguf_path, "gguf").await?;

        self.emit_progress(LlmDownloadProgress {
            phase: "complete",
            bytes_downloaded: 0,
            bytes_total: 0,
            message: "Download complete.".into(),
        });
        Ok(())
    }

    /// Single-file streaming download with `reqwest`. Writes to
    /// `<dest>.partial`, then renames on success so a partial file
    /// from a crash never gets misinterpreted as ready. Emits a
    /// progress event roughly every 1% of the download or every
    /// 256 KB for files where we don't know the total size.
    async fn stream_download(
        &self,
        url: &str,
        dest: &std::path::Path,
        phase: &'static str,
    ) -> AppResult<()> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60 * 60))
            .build()?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|err| AppError::Invalid(format!("Request failed for {url}: {err}")))?;
        if !response.status().is_success() {
            return Err(AppError::Invalid(format!(
                "Download {url} returned HTTP {}",
                response.status()
            )));
        }

        let bytes_total = response.content_length().unwrap_or(0);
        self.emit_progress(LlmDownloadProgress {
            phase,
            bytes_downloaded: 0,
            bytes_total,
            message: format!("Starting {phase} download ({} MB)", bytes_total / (1024 * 1024)),
        });

        let partial_path = dest.with_extension("partial");
        let mut file = tokio::fs::File::create(&partial_path).await.map_err(|err| {
            AppError::Invalid(format!(
                "Failed to create {}: {err}",
                partial_path.display()
            ))
        })?;

        let mut bytes_downloaded: u64 = 0;
        let mut last_emitted_bytes: u64 = 0;
        let emit_interval: u64 = if bytes_total > 0 {
            (bytes_total / 100).max(64 * 1024)
        } else {
            256 * 1024
        };

        let mut response = response;
        loop {
            let next = response.chunk().await.map_err(|err| {
                AppError::Invalid(format!("Stream read failed for {url}: {err}"))
            })?;
            let Some(chunk) = next else {
                break;
            };
            file.write_all(&chunk).await.map_err(|err| {
                AppError::Invalid(format!(
                    "Write failed for {}: {err}",
                    partial_path.display()
                ))
            })?;
            bytes_downloaded += chunk.len() as u64;
            if bytes_downloaded - last_emitted_bytes >= emit_interval {
                last_emitted_bytes = bytes_downloaded;
                self.emit_progress(LlmDownloadProgress {
                    phase,
                    bytes_downloaded,
                    bytes_total,
                    message: String::new(),
                });
            }
        }

        file.flush().await.map_err(|err| {
            AppError::Invalid(format!(
                "Flush failed for {}: {err}",
                partial_path.display()
            ))
        })?;
        drop(file);

        tokio::fs::rename(&partial_path, dest).await.map_err(|err| {
            AppError::Invalid(format!(
                "Rename failed {} → {}: {err}",
                partial_path.display(),
                dest.display()
            ))
        })?;

        self.emit_progress(LlmDownloadProgress {
            phase,
            bytes_downloaded,
            bytes_total: bytes_total.max(bytes_downloaded),
            message: format!("{phase} download complete"),
        });
        Ok(())
    }

    fn emit_progress(&self, payload: LlmDownloadProgress) {
        let _ = self.app.emit("recall://llm-download-progress", payload);
    }

    /// Load weights into memory. Caller holds the state mutex while
    /// this runs so concurrent calls serialize. Loading is on a
    /// blocking thread because mmap'ing a multi-GB GGUF + parsing
    /// tensor metadata isn't free.
    async fn load_into(&self) -> AppResult<LoadedModel> {
        let gguf_path = self.gguf_path()?;
        let backend = self.backend.clone();

        tokio::task::spawn_blocking(move || -> AppResult<LoadedModel> {
            let model_params = LlamaModelParams::default();
            let model = LlamaModel::load_from_file(&backend, &gguf_path, &model_params)
                .map_err(|err| {
                    AppError::Invalid(format!(
                        "llama.cpp model load failed for {}: {err}",
                        gguf_path.display()
                    ))
                })?;
            Ok(LoadedModel { model })
        })
        .await
        .map_err(|err| AppError::Invalid(format!("LLM load task panicked: {err}")))?
    }
}

#[async_trait]
impl AskRecallAdapter for LlamaQwen2Adapter {
    fn model_id(&self) -> LlmModelId {
        self.entry.model_id
    }

    fn hf_repo(&self) -> &'static str {
        self.entry.gguf_repo
    }

    async fn is_ready(&self) -> bool {
        match self.gguf_path() {
            Ok(p) => p.exists(),
            Err(_) => false,
        }
    }

    async fn prepare(&self) -> AppResult<()> {
        self.ensure_files_downloaded().await
    }

    async fn generate_streaming(
        &self,
        request: LlmGenerationRequest,
        on_token: Box<dyn Fn(String) + Send + Sync>,
    ) -> AppResult<LlmGenerationResponse> {
        self.generate_inner(request, Some(on_token)).await
    }

    async fn generate(
        &self,
        request: LlmGenerationRequest,
    ) -> AppResult<LlmGenerationResponse> {
        self.generate_inner(request, None).await
    }

    async fn unload(&self) -> AppResult<()> {
        let mut guard = self.state.lock().await;
        *guard = None;
        Ok(())
    }
}

impl LlamaQwen2Adapter {
    /// Single internal generation path used by both batch and
    /// streaming entry points. The streaming caller supplies an
    /// `on_token` callback; batch passes None and we collect tokens
    /// internally.
    async fn generate_inner(
        &self,
        request: LlmGenerationRequest,
        on_token: Option<Box<dyn Fn(String) + Send + Sync>>,
    ) -> AppResult<LlmGenerationResponse> {
        self.ensure_files_downloaded().await?;
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.load_into().await?);
        }
        // We need the model for the generation thread. Move it out
        // of the mutex temporarily — concurrent callers will block
        // on the mutex which is correct (single-flight inference).
        let LoadedModel { model } = guard.take().expect("just loaded");

        // Wrap the prompt in Qwen2.5's chat template — same shape
        // as v0.4.x. llama.cpp's tokenizer recognises the
        // `<|im_start|>` / `<|im_end|>` special tokens via the
        // tokenizer config baked into the GGUF.
        let formatted = format!(
            "<|im_start|>system\nYou are Recall, a helpful AI assistant grounded in the user's saved memories. Cite each factual claim with [memory:<id>]. If the provided context doesn't contain the answer, say so explicitly — do not guess.<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            request.prompt
        );

        let max_tokens = request.max_tokens.max(1) as i32;
        let temperature = request.temperature.max(0.0);
        let backend = self.backend.clone();

        // Run the whole generation inside spawn_blocking. The
        // callback is captured into the closure; per-token emission
        // happens from the worker thread, which is fine — Tauri's
        // emit() is internally Send + Sync.
        let (response, returned_model) = tokio::task::spawn_blocking(move || -> AppResult<(LlmGenerationResponse, LlamaModel)> {
            let started_at = std::time::Instant::now();

            // Context: reuses model weights but holds the KV cache
            // for one call. Fresh context per generation keeps
            // semantics simple — no accidental cross-call state.
            let n_threads = num_cpus::get_physical().max(1) as i32;
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(CONTEXT_WINDOW_TOKENS))
                .with_n_threads(n_threads)
                .with_n_threads_batch(n_threads);
            let mut ctx = model
                .new_context(&backend, ctx_params)
                .map_err(|err| AppError::Invalid(format!("ctx creation failed: {err}")))?;

            // Tokenize. AddBos::Always mirrors what Qwen2.5 chat
            // expects. Tokenizer special-token handling is built
            // into llama.cpp; we don't need to map sentinels.
            let tokens: Vec<LlamaToken> = model
                .str_to_token(&formatted, AddBos::Always)
                .map_err(|err| AppError::Invalid(format!("tokenize failed: {err}")))?;
            if tokens.len() as u32 >= CONTEXT_WINDOW_TOKENS {
                return Err(AppError::Invalid(format!(
                    "Prompt is too long: {} tokens, context window is {}",
                    tokens.len(),
                    CONTEXT_WINDOW_TOKENS
                )));
            }

            // Prefill: feed the entire prompt as one batched decode.
            // This is the speedup over candle — llama.cpp's SIMD
            // prefill kernels process the prompt at 50–150 tok/s
            // vs ~2.6 tok/s on candle CPU.
            let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
            for (i, token) in tokens.iter().enumerate() {
                let is_last = i == tokens.len() - 1;
                batch
                    .add(*token, i as i32, &[0], is_last)
                    .map_err(|err| AppError::Invalid(format!("batch add failed: {err}")))?;
            }
            ctx.decode(&mut batch)
                .map_err(|err| AppError::Invalid(format!("prefill decode failed: {err}")))?;

            // Sampler: greedy when temperature == 0 (default for v0.4.x
            // and v0.5.0 — reproducibility matters more than diversity
            // for Q&A grounding). Above 0, temperature sampling.
            let mut sampler = if temperature <= 0.0 {
                LlamaSampler::greedy()
            } else {
                LlamaSampler::temp(temperature)
            };

            let mut generated_tokens: Vec<LlamaToken> = Vec::with_capacity(max_tokens as usize);
            let mut emitted_chars: usize = 0;
            let mut answer = String::new();
            let mut n_cur = batch.n_tokens();

            for _ in 0..max_tokens {
                // Sample from the logits of the last token in the
                // most recent decode batch.
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);
                if model.is_eog_token(token) {
                    break;
                }

                // Decode the token to bytes and append. Special::Plaintext
                // strips llama.cpp's chat-template sentinel tokens so we
                // never leak `<|im_end|>` etc. to the UI.
                let piece = model
                    .token_to_bytes(token, Special::Plaintext)
                    .map_err(|err| AppError::Invalid(format!("token decode failed: {err}")))?;
                let piece_str = String::from_utf8_lossy(&piece).into_owned();
                answer.push_str(&piece_str);
                generated_tokens.push(token);

                // Streaming delta: emit just the new chars since
                // last emit. UTF-8 boundaries make per-byte accumulation
                // unsafe; we work on the running `answer` which is
                // always valid UTF-8 because String::push_str enforces it.
                if let Some(callback) = on_token.as_ref() {
                    if answer.len() > emitted_chars {
                        let delta = answer[emitted_chars..].to_string();
                        emitted_chars = answer.len();
                        if !delta.is_empty() {
                            callback(delta);
                        }
                    }
                }

                // Feed the new token back as the next decode input.
                batch.clear();
                batch
                    .add(token, n_cur, &[0], true)
                    .map_err(|err| AppError::Invalid(format!("decode batch add failed: {err}")))?;
                n_cur += 1;
                ctx.decode(&mut batch)
                    .map_err(|err| AppError::Invalid(format!("decode failed: {err}")))?;
            }

            // `ctx` borrows `model`, so we must drop it before
            // moving `model` out into the return tuple. Without
            // this explicit drop, rustc's drop-order rule keeps
            // ctx alive until the end of the closure scope and the
            // borrow checker (correctly) refuses the move.
            drop(ctx);

            Ok((
                LlmGenerationResponse {
                    text: answer.trim().to_string(),
                    latency_ms: started_at.elapsed().as_millis() as u64,
                    tokens_generated: generated_tokens.len() as u32,
                },
                model,
            ))
        })
        .await
        .map_err(|err| AppError::Invalid(format!("LLM gen task panicked: {err}")))??;

        // Restore the model into the mutex so the next generate()
        // doesn't have to reload from disk.
        *guard = Some(LoadedModel {
            model: returned_model,
        });

        Ok(response)
    }
}

/// Erase the concrete adapter into the trait object the rest of the
/// system carries. Keeps callers from having to know which engine is
/// active.
pub fn boxed(app: AppHandle, entry: LlmModelEntry) -> AppResult<Arc<dyn AskRecallAdapter>> {
    Ok(Arc::new(LlamaQwen2Adapter::new(app, entry)?))
}
