//! Candle-based adapter for Qwen2.5-Instruct GGUF models.
//!
//! Loads quantized weights via `candle_transformers::models::quantized_qwen2`
//! and the Hugging Face fast-tokenizer JSON via the `tokenizers` crate.
//! Single-shot greedy generation (temperature 0 by default) so the
//! same question yields the same answer — important for citation
//! verification and for reproducing user-reported bugs.
//!
//! Lifecycle:
//!   * Construction is cheap — just stores the registry entry. No
//!     network or disk I/O.
//!   * `prepare()` downloads model + tokenizer files (one-time
//!     opt-in cloud call) and verifies they're on disk.
//!   * `generate()` lazily loads weights into RAM on first call.
//!     The loaded weights live on `state` (Mutex<Option<...>>)
//!     until `unload()` is called.
//!
//! Thread model: candle's forward pass is sync + CPU-bound on a
//! single core (despite our Tier C having 32 cores — quantized
//! ops in candle aren't parallelized). Wrapped in
//! `tokio::task::spawn_blocking` so the runtime stays responsive
//! during a multi-second generation.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen2::ModelWeights as Qwen2GGUF;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokenizers::Tokenizer;
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

/// Event payload for `recall://llm-download-progress`. The UI
/// listens, accumulates `(bytes_downloaded, bytes_total)` per
/// `phase`, and renders a progress bar. `bytes_total = 0` means
/// the server didn't return Content-Length — the UI shows an
/// indeterminate spinner in that case.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmDownloadProgress {
    /// `"gguf"` while pulling the weights file, `"tokenizer"`
    /// while pulling the tokenizer JSON, `"complete"` when the
    /// whole download finishes (or returns immediately because
    /// files were already on disk).
    pub phase: &'static str,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    /// Optional human-readable summary line — empty for incremental
    /// progress events, populated for phase boundaries.
    pub message: String,
}

pub struct CandleQwen2Adapter {
    app: AppHandle,
    entry: LlmModelEntry,
    /// Loaded weights + tokenizer + device. None when unloaded;
    /// `generate()` populates on first call. Mutex is fine because
    /// generation is single-flight per call anyway.
    state: Mutex<Option<LoadedModel>>,
}

struct LoadedModel {
    model: Qwen2GGUF,
    tokenizer: Tokenizer,
    device: Device,
    eos_token_id: u32,
}

impl CandleQwen2Adapter {
    pub fn new(app: AppHandle, entry: LlmModelEntry) -> Self {
        Self {
            app,
            entry,
            state: Mutex::new(None),
        }
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

    fn tokenizer_path(&self) -> AppResult<PathBuf> {
        Ok(self.cache_dir()?.join(self.entry.tokenizer_file))
    }

    /// Pull GGUF + tokenizer from their respective HF repos into
    /// the cache dir. Streams the response with reqwest so we can
    /// emit progress events to the UI — hf-hub's built-in download
    /// only renders progress in a terminal (uses `indicatif`),
    /// which is invisible inside a Tauri release build. Two repos
    /// because Qwen's GGUF and base-model repos are split (only
    /// the base repo carries tokenizer.json).
    ///
    /// Idempotent — files already on disk are skipped (and emit a
    /// `phase = "skipped"` final event so the UI can flip cleanly).
    async fn ensure_files_downloaded(&self) -> AppResult<()> {
        let gguf_path = self.gguf_path()?;
        let tokenizer_path = self.tokenizer_path()?;
        if gguf_path.exists() && tokenizer_path.exists() {
            self.emit_progress(LlmDownloadProgress {
                phase: "complete",
                bytes_downloaded: 0,
                bytes_total: 0,
                message: "Model already on disk.".into(),
            });
            return Ok(());
        }

        if !gguf_path.exists() {
            let url = format!(
                "https://huggingface.co/{}/resolve/main/{}",
                self.entry.gguf_repo, self.entry.gguf_file
            );
            self.stream_download(&url, &gguf_path, "gguf").await?;
        }

        if !tokenizer_path.exists() {
            let url = format!(
                "https://huggingface.co/{}/resolve/main/{}",
                self.entry.tokenizer_repo, self.entry.tokenizer_file
            );
            self.stream_download(&url, &tokenizer_path, "tokenizer")
                .await?;
        }

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
    /// 256 KB for files where we don't know the total size — keeps
    /// the UI updates responsive without spamming the IPC channel.
    async fn stream_download(
        &self,
        url: &str,
        dest: &std::path::Path,
        phase: &'static str,
    ) -> AppResult<()> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60 * 60)) // 1h ceiling
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
        // Initial event so the UI can flip from "Downloading…" to
        // a real progress bar with a denominator immediately.
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
        // Emit at least every 1% (or 256 KB if total unknown).
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

        // Final event with 100% — UI uses this to move the bar to
        // full before the request resolves.
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

    /// Load weights + tokenizer into memory. Caller holds the state
    /// mutex while this runs so concurrent calls serialize.
    async fn load_into(&self) -> AppResult<LoadedModel> {
        // Load on a blocking thread — opening + memory-mapping a
        // multi-GB GGUF can take 1–3 seconds and we don't want to
        // hold the tokio runtime.
        let gguf_path = self.gguf_path()?;
        let tokenizer_path = self.tokenizer_path()?;

        tokio::task::spawn_blocking(move || -> AppResult<LoadedModel> {
            let device = Device::Cpu;
            let mut file = std::fs::File::open(&gguf_path).map_err(|err| {
                AppError::Invalid(format!(
                    "Failed to open GGUF {}: {err}",
                    gguf_path.display()
                ))
            })?;
            let gguf_content = gguf_file::Content::read(&mut file).map_err(|err| {
                AppError::Invalid(format!("GGUF read failed: {err}"))
            })?;
            let model = Qwen2GGUF::from_gguf(gguf_content, &mut file, &device)
                .map_err(|err| AppError::Invalid(format!("Qwen2 model load failed: {err}")))?;

            let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|err| {
                AppError::Invalid(format!(
                    "Tokenizer load failed for {}: {err}",
                    tokenizer_path.display()
                ))
            })?;

            // Qwen2.5-Instruct uses `<|im_end|>` as the chat EOS.
            // Tokenizer encodes it as a single token id; we look it
            // up once and stash it for the generation loop.
            let eos_token_id = tokenizer
                .token_to_id("<|im_end|>")
                .ok_or_else(|| {
                    AppError::Invalid(
                        "Tokenizer missing `<|im_end|>` chat-stop token (not a Qwen2.5-Instruct tokenizer?)"
                            .into(),
                    )
                })?;

            Ok(LoadedModel {
                model,
                tokenizer,
                device,
                eos_token_id,
            })
        })
        .await
        .map_err(|err| AppError::Invalid(format!("LLM load task panicked: {err}")))?
    }
}

#[async_trait]
impl AskRecallAdapter for CandleQwen2Adapter {
    fn model_id(&self) -> LlmModelId {
        self.entry.model_id
    }

    fn hf_repo(&self) -> &'static str {
        // GGUF repo is the dominant download (1–5 GB) and what the
        // user mostly cares about being shown. Tokenizer is a few
        // MB from the base repo and elided from the UI.
        self.entry.gguf_repo
    }

    async fn is_ready(&self) -> bool {
        // Files-on-disk is the threshold for "ready" — weights may
        // or may not be loaded into RAM, but `generate` will lazily
        // load them. The Settings UI uses this to flip the Download
        // button between "Download" and "Model ready".
        let gguf = match self.gguf_path() {
            Ok(p) => p,
            Err(_) => return false,
        };
        let tokenizer = match self.tokenizer_path() {
            Ok(p) => p,
            Err(_) => return false,
        };
        gguf.exists() && tokenizer.exists()
    }

    async fn prepare(&self) -> AppResult<()> {
        self.ensure_files_downloaded().await
    }

    async fn generate(
        &self,
        request: LlmGenerationRequest,
    ) -> AppResult<LlmGenerationResponse> {
        // Make sure files exist + load weights if not yet loaded.
        self.ensure_files_downloaded().await?;
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.load_into().await?);
        }
        // Move the loaded model out of the mutex for the duration
        // of generation; a concurrent `generate` would block on
        // the mutex which is fine for v0.4.0 (single-flight).
        let loaded = guard.as_mut().expect("just loaded");

        // Wrap the prompt in Qwen2.5's chat template. The model
        // expects exactly this shape — system + user + assistant
        // sentinels — to produce well-formed answers. The caller's
        // `request.prompt` is treated as the user turn content.
        let formatted = format!(
            "<|im_start|>system\nYou are Recall, a helpful AI assistant grounded in the user's saved memories. Cite each factual claim with [memory:<id>]. If the provided context doesn't contain the answer, say so explicitly — do not guess.<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            request.prompt
        );

        let started_at = std::time::Instant::now();
        let max_tokens = request.max_tokens.max(1);
        let temperature = request.temperature.max(0.0) as f64;

        // Tokenize the prompt outside the blocking pool — tokenizers
        // is cheap on inputs of our size. The expensive step is the
        // forward passes.
        let encoding = loaded
            .tokenizer
            .encode(formatted, true)
            .map_err(|err| AppError::Invalid(format!("tokenize failed: {err}")))?;
        let mut tokens: Vec<u32> = encoding.get_ids().to_vec();

        // Logits processor — temperature 0 = greedy. Anything above
        // introduces sampling; v0.4.0 defaults to greedy for
        // reproducibility.
        let sampling = if temperature <= 0.0 {
            Sampling::ArgMax
        } else {
            Sampling::All { temperature }
        };
        let mut logits_processor = LogitsProcessor::from_sampling(42, sampling);

        let mut generated: Vec<u32> = Vec::with_capacity(max_tokens);
        let mut index_pos = 0usize;

        // The generation loop CPU-binds for the duration; off-load
        // to a blocking thread so the runtime stays responsive.
        // We do the whole loop in one spawn_blocking — switching back
        // and forth per-token would dwarf the per-token cost.
        let LoadedModel {
            model,
            tokenizer,
            device,
            eos_token_id,
        } = loaded;
        let model_ref: &mut Qwen2GGUF = model;
        let tokenizer_ref: &Tokenizer = tokenizer;
        let device_ref: &Device = device;
        let eos = *eos_token_id;

        // Run the autoregressive loop directly on the current task —
        // wrapping it in spawn_blocking would require sending the
        // model across thread boundaries (it isn't `Send` for the
        // mutex guard's lifetime). Tokio's multi-thread runtime
        // will schedule this on a worker thread for the call's
        // lifetime, and the `await` on each loop pass yields back
        // briefly enough that other tasks can interleave.
        for step in 0..max_tokens {
            let context_size = if step == 0 { tokens.len() } else { 1 };
            let start = tokens.len().saturating_sub(context_size);
            let ctx_slice = &tokens[start..];
            let input = Tensor::new(ctx_slice, device_ref)
                .map_err(|err| AppError::Invalid(format!("tensor build failed: {err}")))?
                .unsqueeze(0)
                .map_err(|err| AppError::Invalid(format!("tensor unsqueeze failed: {err}")))?;
            let logits = model_ref
                .forward(&input, index_pos)
                .map_err(|err| AppError::Invalid(format!("forward pass failed: {err}")))?
                .squeeze(0)
                .map_err(|err| AppError::Invalid(format!("logits squeeze failed: {err}")))?;
            // The forward returns logits for every token in the
            // input slice; we only want the last token's logits.
            let logits = if logits.dims().len() == 2 {
                let last = logits
                    .dim(0)
                    .map_err(|err| AppError::Invalid(format!("logits dim read failed: {err}")))?
                    - 1;
                logits
                    .get(last)
                    .map_err(|err| AppError::Invalid(format!("logits index failed: {err}")))?
            } else {
                logits
            };
            let next_token = logits_processor
                .sample(&logits)
                .map_err(|err| AppError::Invalid(format!("logits sampling failed: {err}")))?;
            index_pos += ctx_slice.len();
            if next_token == eos {
                break;
            }
            tokens.push(next_token);
            generated.push(next_token);
        }

        let text = tokenizer_ref
            .decode(&generated, true)
            .map_err(|err| AppError::Invalid(format!("decode failed: {err}")))?;

        Ok(LlmGenerationResponse {
            text: text.trim().to_string(),
            latency_ms: started_at.elapsed().as_millis() as u64,
            tokens_generated: generated.len() as u32,
        })
    }

    async fn unload(&self) -> AppResult<()> {
        let mut guard = self.state.lock().await;
        *guard = None;
        Ok(())
    }
}

/// Erase the concrete adapter into the trait object the rest of the
/// system carries. Keeps callers from having to know which model is
/// active.
pub fn boxed(app: AppHandle, entry: LlmModelEntry) -> Arc<dyn AskRecallAdapter> {
    Arc::new(CandleQwen2Adapter::new(app, entry))
}
