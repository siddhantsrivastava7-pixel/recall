// AI Settings tab — v0.2.0 (Phase 1: Foundation + Native OCR).
//
// Ruthlessly minimal per the locked PRD. Six controls, in this order:
//
//   1. AI master toggle (off by default)
//   2. Detected hardware tier (read-only)
//   3. OCR engine readout (read-only)
//   4. Pause on battery toggle
//   5. Run heavy tasks only while plugged in toggle
//   6. Run OCR rebuild button
//
// No model storage path, no advanced AI mode picker, no unload-model
// settings. Those land in v0.3.0+ when the features behind them exist.

import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Sparkles,
  Cpu,
  Eye,
  RefreshCw,
  AlertCircle,
  CheckCircle,
  Clipboard,
  Download,
  Layers,
  Timer,
  Zap,
} from "lucide-react";
import {
  aiClient,
  type ClipboardImageDiagnostic,
  type LlmDiagnosticPayload,
  type LlmDownloadProgress,
  type LlmStatusPayload,
} from "@/services/ai/AiClient";
import { useSettingsStore } from "@/stores/settingsStore";
import type { AiStatusPayload } from "@/domain/types";

/// Poll cadence for the AI status snapshot while this tab is mounted.
/// 2s is fast enough that "Embed all memories" feels live without
/// hammering SQLite — the underlying query is a few COUNT aggregates.
const STATUS_POLL_INTERVAL_MS = 2000;

/// Tauri's `invoke` rejects with whatever Rust returns from `Err(_)`.
/// AppError serializes to a string (see `errors/app_error.rs`), so the
/// promise rejection arrives as a plain string — `error instanceof Error`
/// is false in that case, which would silently swallow the actual
/// reason. Always use this helper inside `catch` blocks so the user
/// sees the real message.
function describeError(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    try {
      return JSON.stringify(error);
    } catch {
      return fallback;
    }
  }
  return fallback;
}

type Notice =
  | { kind: "idle" }
  | { kind: "info"; message: string }
  | { kind: "error"; message: string };

const GB_BYTES = 1024 * 1024 * 1024;

function formatRam(bytes: number) {
  if (bytes <= 0) return "—";
  const gb = bytes / GB_BYTES;
  return gb >= 10 ? `${gb.toFixed(0)} GB` : `${gb.toFixed(1)} GB`;
}

function tierLabel(tier: AiStatusPayload["hardware"]["tier"]) {
  switch (tier) {
    case "a":
      return "Tier A · 8 GB class";
    case "b":
      return "Tier B · 16 GB class";
    case "c":
      return "Tier C · 32 GB class";
    default:
      return "Unknown";
  }
}

function formatProgress(progress: LlmDownloadProgress): string {
  const downloadedMb = progress.bytesDownloaded / (1024 * 1024);
  if (progress.bytesTotal === 0) {
    return `${downloadedMb.toFixed(0)} MB`;
  }
  const totalMb = progress.bytesTotal / (1024 * 1024);
  const pct = (progress.bytesDownloaded / progress.bytesTotal) * 100;
  if (totalMb >= 1024) {
    return `${(downloadedMb / 1024).toFixed(2)} / ${(totalMb / 1024).toFixed(2)} GB · ${pct.toFixed(0)}%`;
  }
  return `${downloadedMb.toFixed(0)} / ${totalMb.toFixed(0)} MB · ${pct.toFixed(0)}%`;
}

function hasStaleNamespaceEmbeddings(status: AiStatusPayload): boolean {
  const cov = status.embeddingCoverage;
  // We have stale namespace rows if there are total embedded chunks
  // out there, but the active model accounts for fewer of them. In
  // a fresh install both numbers are equal (or both zero).
  return cov.embeddedChunks > cov.embeddedChunksActiveModel;
}

function ocrEngineLabel(engine: string) {
  switch (engine) {
    case "apple-vision":
      return "Apple Vision";
    case "windows-media-ocr":
      return "Windows.Media.Ocr";
    case "unsupported":
      return "Not available on this OS";
    default:
      return engine;
  }
}

export function AiSettingsTab() {
  const { settings, updateSettings } = useSettingsStore();
  const [status, setStatus] = useState<AiStatusPayload | null>(null);
  const [notice, setNotice] = useState<Notice>({ kind: "idle" });
  const [busy, setBusy] = useState<
    | "toggle"
    | "rebuild"
    | "diagnose"
    | "downloadModel"
    | "embedAll"
    | "downloadLlm"
    | "unloadLlm"
    | "diagnoseLlm"
    | "scrub"
    | null
  >(null);
  const [llmStatus, setLlmStatus] = useState<LlmStatusPayload | null>(null);
  const [llmDiagnostic, setLlmDiagnostic] = useState<LlmDiagnosticPayload | null>(null);
  const [llmProgress, setLlmProgress] = useState<LlmDownloadProgress | null>(null);
  const [diagnostic, setDiagnostic] = useState<ClipboardImageDiagnostic | null>(null);

  // Live status: poll on a short tick so the chunk-embed coverage
  // counter and queue badges actually update while you watch the tab.
  // The Tauri scheduler also emits `recall://memory-ocr-updated` and
  // `recall://memory-embedding-updated` — we listen on both as a
  // push-based fast path in addition to the polling safety net.
  // Component unmount stops the timer; switching tabs in the parent
  // settings view unmounts this component, so polling never runs in
  // the background.
  const lastFetchAt = useRef<number>(0);
  useEffect(() => {
    let cancelled = false;

    async function refresh() {
      // Coalesce: ignore refreshes within 250ms of each other so a
      // burst of `memory-embedding-updated` events while the worker
      // pool drains a queue doesn't fan out to N concurrent reads.
      const now = Date.now();
      if (now - lastFetchAt.current < 250) return;
      lastFetchAt.current = now;
      try {
        const next = await aiClient.status();
        if (!cancelled) setStatus(next);
      } catch (error) {
        if (cancelled) return;
        const message = describeError(error, "Unable to read AI status.");
        setNotice((current) =>
          current.kind === "error" ? current : { kind: "error", message },
        );
      }
    }

    void refresh();

    const interval = setInterval(() => void refresh(), STATUS_POLL_INTERVAL_MS);
    const ocrUnlistenPromise = listen("recall://memory-ocr-updated", () => void refresh());
    const embedUnlistenPromise = listen(
      "recall://memory-embedding-updated",
      () => void refresh(),
    );

    return () => {
      cancelled = true;
      clearInterval(interval);
      void ocrUnlistenPromise.then((dispose) => dispose());
      void embedUnlistenPromise.then((dispose) => dispose());
    };
  }, [settings.aiEnabled]);

  const handleToggleEnabled = async (next: boolean) => {
    setBusy("toggle");
    setNotice({ kind: "idle" });
    try {
      const updated = await aiClient.setEnabled(next);
      setStatus(updated);
      // Mirror the flip into the settings store so other components
      // (e.g. the General tab, future status badges) see it without
      // an extra round trip.
      await updateSettings({ ...settings, aiEnabled: updated.enabled });
    } catch (error) {
      const message = describeError(error, "Unable to toggle AI.");
      setNotice({ kind: "error", message });
    } finally {
      setBusy(null);
    }
  };

  const handleEmbedAll = async () => {
    setBusy("embedAll");
    setNotice({ kind: "idle" });
    try {
      const summary = await aiClient.embedAllMemories();
      const queued = summary.chunksEnqueued;
      const reset = summary.failedJobsReset;
      const parts: string[] = [];
      if (summary.memoriesChunked > 0) {
        parts.push(
          `chunked ${summary.memoriesChunked.toLocaleString()} ${
            summary.memoriesChunked === 1 ? "memory" : "memories"
          } into ${summary.chunksCreated.toLocaleString()} ${
            summary.chunksCreated === 1 ? "chunk" : "chunks"
          }`,
        );
      }
      if (queued > 0) {
        parts.push(`queued ${queued.toLocaleString()} for embedding`);
      }
      if (reset > 0) {
        parts.push(`reset ${reset.toLocaleString()} stuck job${reset === 1 ? "" : "s"}`);
      }
      const message =
        parts.length === 0
          ? "Already up to date — every chunk has an embedding."
          : `Embedding pass: ${parts.join(", ")}.`;
      setNotice({ kind: "info", message });
      const refreshed = await aiClient.status();
      setStatus(refreshed);
    } catch (error) {
      const message = describeError(error, "Failed to start embedding pass.");
      setNotice({ kind: "error", message });
    } finally {
      setBusy(null);
    }
  };

  // v0.4.0a: pull initial LLM status on mount (and when tab is
  // switched back to). Cheap — no model load, just disk-presence
  // check + registry lookup.
  // v0.4.2: also listen for download-progress events from the
  // backend so the UI can render a real progress bar during the
  // multi-GB GGUF download instead of just "Downloading…".
  useEffect(() => {
    let cancelled = false;
    aiClient
      .llmStatus()
      .then((s) => {
        if (!cancelled) setLlmStatus(s);
      })
      .catch(() => {
        // LLM adapter not configured on this host — leave llmStatus null,
        // the UI hides the section.
      });

    const unlistenPromise = listen<LlmDownloadProgress>(
      "recall://llm-download-progress",
      (event) => {
        if (cancelled) return;
        const payload = event.payload;
        setLlmProgress(payload);
        if (payload.phase === "complete") {
          // Refresh ready state so the button flips to "Model ready".
          aiClient
            .llmStatus()
            .then((s) => {
              if (!cancelled) setLlmStatus(s);
            })
            .catch(() => {});
        }
      },
    );

    return () => {
      cancelled = true;
      void unlistenPromise.then((dispose) => dispose());
    };
  }, []);

  const handleDownloadLlm = async () => {
    setBusy("downloadLlm");
    setNotice({
      kind: "info",
      message:
        "Downloading the Ask Recall model — this can take a few minutes (1–4 GB depending on your tier). One-time setup; future runs are offline.",
    });
    try {
      await aiClient.downloadLlm();
      const fresh = await aiClient.llmStatus();
      setLlmStatus(fresh);
      setNotice({
        kind: "info",
        message:
          "Ask Recall model is ready. Run the smoke test below to verify inference works on this machine.",
      });
    } catch (error) {
      setNotice({
        kind: "error",
        message: describeError(error, "Failed to download Ask Recall model."),
      });
    } finally {
      setBusy(null);
    }
  };

  const handleUnloadLlm = async () => {
    setBusy("unloadLlm");
    try {
      await aiClient.unloadLlm();
      setNotice({
        kind: "info",
        message: "Model unloaded from RAM. Next question will reload it from disk.",
      });
    } catch (error) {
      setNotice({
        kind: "error",
        message: describeError(error, "Failed to unload Ask Recall model."),
      });
    } finally {
      setBusy(null);
    }
  };

  const handleDiagnoseLlm = async () => {
    setBusy("diagnoseLlm");
    setLlmDiagnostic(null);
    try {
      const result = await aiClient.diagnoseLlm();
      setLlmDiagnostic(result);
    } catch (error) {
      setLlmDiagnostic({
        ok: false,
        modelId: llmStatus?.modelId ?? "",
        prompt: "",
        response: "",
        tokensGenerated: 0,
        latencyMs: 0,
        message: describeError(error, "Smoke test failed."),
      });
    } finally {
      setBusy(null);
    }
  };

  const handleDownloadEmbeddingModel = async () => {
    setBusy("downloadModel");
    setNotice({
      kind: "info",
      message: "Downloading the embedding model (~30 MB). This is a one-time setup.",
    });
    try {
      await aiClient.downloadEmbeddingModel();
      setNotice({
        kind: "info",
        message: "Embedding model is ready. Existing memories will start embedding in the background.",
      });
      const refreshed = await aiClient.status();
      setStatus(refreshed);
    } catch (error) {
      const message = describeError(error, "Failed to download embedding model.");
      setNotice({ kind: "error", message });
    } finally {
      setBusy(null);
    }
  };

  const handleDiagnose = async () => {
    setBusy("diagnose");
    setNotice({ kind: "idle" });
    try {
      const result = await aiClient.diagnoseClipboardImage();
      setDiagnostic(result);
    } catch (error) {
      const message = describeError(error, "Unable to diagnose clipboard.");
      setDiagnostic({ ok: false, message });
    } finally {
      setBusy(null);
    }
  };

  const handleRebuild = async () => {
    setBusy("rebuild");
    setNotice({ kind: "idle" });
    try {
      const queued = await aiClient.rebuildIndex();
      if (queued === 0) {
        setNotice({
          kind: "info",
          message: "Nothing to do — every eligible memory already has OCR queued or done.",
        });
      } else {
        setNotice({
          kind: "info",
          message: `Queued OCR for ${queued} ${queued === 1 ? "memory" : "memories"}.`,
        });
      }
      const refreshed = await aiClient.status();
      setStatus(refreshed);
    } catch (error) {
      const message = describeError(error, "Unable to rebuild OCR index.");
      setNotice({ kind: "error", message });
    } finally {
      setBusy(null);
    }
  };

  /// v0.5.8: manual scrub trigger — recovery path when the boot-time
  /// auto-backfill silently fails. Returns counts so the user (and
  /// us, debugging) can see exactly what changed.
  /// v0.5.9: extended to render before/after counts per managed
  /// tag, plus the brute-force SQL bulk-purge counter, so the
  /// audit unambiguously shows whether stale tags actually
  /// disappeared from the DB.
  const handleForceScrub = async () => {
    setBusy("scrub");
    setNotice({ kind: "idle" });
    try {
      const result = await aiClient.forceScrub();
      // Build a delta line per managed tag where the before/after
      // numbers differ. Surfaces cases like "license-key 12 → 0"
      // distinctly from cases like "url 47 → 47" (no change because
      // those URLs legitimately have URLs in their content).
      const tagOrder = [
        "license-key",
        "url",
        "email",
        "phone-number",
        "ip-address",
        "code-snippet",
        "hash",
      ];
      const deltas = tagOrder
        .map((tag) => {
          const before = result.beforeCounts[tag] ?? 0;
          const after = result.afterCounts[tag] ?? 0;
          if (before === 0 && after === 0) return null;
          const arrow = before === after ? "=" : "→";
          return `${tag}: ${before} ${arrow} ${after}`;
        })
        .filter((line): line is string => line !== null);
      const auditLine = deltas.length > 0 ? ` Tags: ${deltas.join("; ")}.` : "";
      setNotice({
        kind: "info",
        message: `Scrubbed ${result.memoriesScanned} memories: ${result.bulkPurgeRowsAffected} rows bulk-purged, ${result.selfCapturesMarked} self-captures flagged, ${result.entitiesExtracted} entities extracted${result.errors > 0 ? `, ${result.errors} errors` : ""} (${(result.elapsedMs / 1000).toFixed(1)}s).${auditLine}`,
      });
    } catch (error) {
      const message = describeError(error, "Unable to re-scrub AI tags.");
      setNotice({ kind: "error", message });
    } finally {
      setBusy(null);
    }
  };

  const enabled = status?.enabled ?? settings.aiEnabled;
  const ocrAvailable = status?.ocrAvailable ?? false;
  const queue = status?.queue;

  return (
    <Section title="AI">
      <div
        style={{
          fontSize: 13,
          color: "var(--text-muted)",
          marginBottom: 16,
          maxWidth: 540,
          lineHeight: 1.55,
        }}
      >
        Recall&rsquo;s AI runs entirely on your machine. Off by default. When enabled,
        it adds OCR to screenshots in the background so they become searchable.
        No data leaves your device.
      </div>

      <Toggle
        label="Enable on-device AI"
        description="Master switch. Turning this off drains the queue and stops new work."
        value={enabled}
        disabled={busy === "toggle"}
        onChange={handleToggleEnabled}
        icon={<Sparkles size={14} />}
      />

      <ReadoutRow
        icon={<Cpu size={14} />}
        label="Detected hardware"
        value={
          status
            ? `${tierLabel(status.hardware.tier)} · ${formatRam(
                status.hardware.totalRamBytes,
              )} · ${status.hardware.cpuCores} cores`
            : "Detecting…"
        }
      />

      <ReadoutRow
        icon={<Eye size={14} />}
        label="OCR engine"
        value={status ? ocrEngineLabel(status.ocrEngine) : "Detecting…"}
        warn={status != null && !ocrAvailable}
      />

      <Toggle
        label="Pause on battery"
        description="Don't claim new AI work while running on battery power."
        value={settings.aiPauseOnBattery}
        onChange={(v) => void updateSettings({ ...settings, aiPauseOnBattery: v })}
      />

      <Toggle
        label="Run heavy tasks only while plugged in"
        description="Limit OCR (and future embedding work) to AC power."
        value={settings.aiHeavyOnlyOnAc}
        onChange={(v) => void updateSettings({ ...settings, aiHeavyOnlyOnAc: v })}
      />

      {/*
        v0.5.22 — low-battery pause threshold. Independent from the
        AC toggles above: a laptop on a struggling charger can still
        drop in percent, and we want to ease off then. `0` disables
        the gate (treat the slider as "never pause based on percent").
        Has no effect on macOS / desktops without battery sensors —
        the throttling layer treats unreadable percent as "no
        constraint" rather than parking the scheduler.
      */}
      <DropdownRow
        icon={<Zap size={14} />}
        label="Pause AI when battery is low"
        description="Stop background OCR + embedding work below this percentage. Independent from the AC-power toggles above. Windows only — macOS doesn't expose battery percent yet."
        value={String(settings.aiPauseBelowBatteryPct)}
        onChange={(value) => {
          const pct = Number.parseInt(value, 10);
          if (Number.isNaN(pct)) return;
          void updateSettings({ ...settings, aiPauseBelowBatteryPct: pct });
        }}
        options={[
          { value: "0", label: "Off" },
          { value: "10", label: "Below 10%" },
          { value: "20", label: "Below 20%" },
          { value: "30", label: "Below 30%" },
          { value: "50", label: "Below 50%" },
        ]}
      />

      {/*
        v0.5.21 — Performance subsection. Two new controls land
        here: idle reaper threshold (live, takes effect within 60s)
        and hardware tier override (requires restart). Anything
        more advanced (model swap UI with download progress) is
        v0.5.22 scope.
      */}
      <div
        style={{
          paddingTop: 24,
          borderTop: "1px solid rgba(255,255,255,0.05)",
          marginTop: 8,
          marginBottom: 8,
        }}
      >
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 14 }}>
          Performance
        </div>

        <DropdownRow
          icon={<Timer size={14} />}
          label="Unload LLM after"
          description="Free RAM by unloading the model after this idle period. Next Ask Recall pays a 5–10s cold reload cost. Takes effect within ~60 seconds — no restart."
          value={String(settings.aiLlmIdleMinutes)}
          onChange={(value) => {
            const minutes = Number.parseInt(value, 10);
            if (Number.isNaN(minutes)) return;
            void updateSettings({ ...settings, aiLlmIdleMinutes: minutes });
          }}
          options={[
            { value: "1", label: "1 minute" },
            { value: "5", label: "5 minutes" },
            { value: "15", label: "15 minutes" },
            { value: "30", label: "30 minutes" },
            { value: "60", label: "1 hour" },
            { value: "0", label: "Never (keep loaded)" },
          ]}
        />

        <DropdownRow
          icon={<Zap size={14} />}
          label="Hardware tier"
          description={
            status
              ? `Auto-detected as ${tierLabel(status.hardware.tier)}. Override to force a specific model size — restart required to apply.`
              : "Override the auto-detected tier to force a specific model size. Restart required to apply."
          }
          value={settings.aiTierOverride ?? "auto"}
          onChange={(value) => {
            const override =
              value === "auto" ? null : (value as "a" | "b" | "c");
            void updateSettings({ ...settings, aiTierOverride: override });
          }}
          options={[
            {
              value: "auto",
              label: status
                ? `Auto (${tierLabel(status.hardware.tier)})`
                : "Auto",
            },
            { value: "a", label: "Tier A — 1.5B model (~1 GB)" },
            { value: "b", label: "Tier B — 3B model (~2 GB)" },
            { value: "c", label: "Tier C — 7B model (~4 GB)" },
          ]}
        />
      </div>

      <div
        style={{
          paddingTop: 24,
          borderTop: "1px solid rgba(255,255,255,0.05)",
          marginTop: 8,
        }}
      >
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 14 }}>
          Embeddings
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-muted)",
            marginBottom: 12,
            maxWidth: 540,
            lineHeight: 1.5,
          }}
        >
          One-time download of a small (~30 MB) embedding model from Hugging Face.
          Powers Related Memories on detail views, and (in a later release)
          semantic search and Ask Recall. Once downloaded, every embedding runs
          fully offline.
        </div>
        <div
          style={{
            display: "flex",
            gap: 10,
            alignItems: "center",
            flexWrap: "wrap",
          }}
        >
          <button
            className="btn-ghost"
            onClick={() => void handleDownloadEmbeddingModel()}
            disabled={!enabled || busy === "downloadModel" || (status?.embeddingReady ?? false)}
          >
            <Download size={13} />
            {busy === "downloadModel"
              ? "Downloading…"
              : status?.embeddingReady
                ? "Model ready"
                : "Download embedding model"}
          </button>
          <button
            className="btn-ghost"
            onClick={() => void handleEmbedAll()}
            disabled={!enabled || !status?.embeddingReady || busy === "embedAll"}
            title={
              !status?.embeddingReady
                ? "Download the embedding model first."
                : "Re-chunk every memory and (re-)embed any that lack vectors. Safe to run any time."
            }
          >
            <RefreshCw size={13} />
            {busy === "embedAll" ? "Embedding…" : "Embed all memories"}
          </button>
          {status ? (
            <span
              style={{
                fontSize: 12,
                color: "var(--text-muted)",
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
              }}
            >
              <Layers size={12} />
              {status.embeddingCoverage.embeddedChunksActiveModel.toLocaleString()} of{" "}
              {status.embeddingCoverage.totalChunks.toLocaleString()} chunks embedded
              {" "}
              <span style={{ color: "var(--t-4)" }}>
                ({status.embeddingModel})
              </span>
              {status.queue.embedQueued > 0
                ? ` · ${status.queue.embedQueued} queued`
                : ""}
              {status.queue.embedRunning > 0
                ? ` · ${status.queue.embedRunning} running`
                : ""}
              {status.queue.embedFailed > 0
                ? ` · ${status.queue.embedFailed} failed`
                : ""}
            </span>
          ) : null}
        </div>
        {status && hasStaleNamespaceEmbeddings(status) ? (
          <div
            style={{
              marginTop: 12,
              padding: "10px 12px",
              borderRadius: 10,
              background: "var(--blue-dim, rgba(64,128,255,0.10))",
              border: "1px solid var(--blue-border, rgba(64,128,255,0.28))",
              color: "var(--text-primary)",
              fontSize: 12,
              lineHeight: 1.5,
              maxWidth: 600,
            }}
          >
            <strong>New embedding model: {status.embeddingModel}.</strong>{" "}
            Older embeddings exist under a previous model namespace and won't
            be used for ranking. Click <em>Embed all memories</em> to upgrade
            them — re-embedding is incremental, so unchanged chunks recompute
            once and cache.
          </div>
        ) : null}
      </div>

      {llmStatus ? (
        <div
          style={{
            paddingTop: 24,
            borderTop: "1px solid rgba(255,255,255,0.05)",
            marginTop: 24,
          }}
        >
          <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 6 }}>
            Ask Recall (preview)
          </div>
          <div
            style={{
              fontSize: 12,
              color: "var(--text-muted)",
              marginBottom: 12,
              maxWidth: 600,
              lineHeight: 1.5,
            }}
          >
            One-time download of a local LLM that answers questions over your
            saved memories. Picked for your tier:{" "}
            <strong style={{ color: "var(--text-primary)" }}>{llmStatus.modelId}</strong>{" "}
            ({(llmStatus.approxDownloadMb / 1024).toFixed(1)} GB download, ~
            {(llmStatus.approxInferenceRamMb / 1024).toFixed(1)} GB RAM at use).
            v0.4.0a is a smoke test — once you've downloaded the model, click "Run
            smoke test" to verify inference works on this machine. The full Ask
            Recall surface (question box on Home, retrieval + citations) lands
            in v0.4.0c.
          </div>
          <div
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              flexWrap: "wrap",
            }}
          >
            <button
              className="btn-ghost"
              onClick={() => void handleDownloadLlm()}
              disabled={!enabled || busy === "downloadLlm" || llmStatus.ready}
            >
              <Download size={13} />
              {busy === "downloadLlm"
                ? "Downloading…"
                : llmStatus.ready
                  ? "Model ready"
                  : `Download Ask Recall model (~${Math.round(
                      llmStatus.approxDownloadMb / 100,
                    ) / 10} GB)`}
            </button>
            <button
              className="btn-ghost"
              onClick={() => void handleDiagnoseLlm()}
              disabled={!enabled || !llmStatus.ready || busy === "diagnoseLlm"}
              title={
                !llmStatus.ready
                  ? "Download the model first."
                  : "Run a fixed prompt end-to-end to verify inference."
              }
            >
              <Sparkles size={13} />
              {busy === "diagnoseLlm" ? "Running…" : "Run smoke test"}
            </button>
            {llmStatus.ready ? (
              <button
                className="btn-ghost"
                onClick={() => void handleUnloadLlm()}
                disabled={busy === "unloadLlm"}
                title="Drop loaded weights from RAM. Next question reloads from disk."
              >
                {busy === "unloadLlm" ? "Unloading…" : "Unload from RAM"}
              </button>
            ) : null}
          </div>

          {/*
            v0.4.2: live progress during download. Shown while
            `busy === "downloadLlm"` AND the backend has emitted
            at least one progress event. Hides itself once the
            phase flips to "complete" so the panel collapses
            cleanly.
          */}
          {busy === "downloadLlm" && llmProgress && llmProgress.phase !== "complete" ? (
            <div style={{ marginTop: 14, maxWidth: 600 }}>
              <div
                style={{
                  fontSize: 12,
                  color: "var(--text-muted)",
                  marginBottom: 6,
                  display: "flex",
                  justifyContent: "space-between",
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                <span>
                  {llmProgress.phase === "gguf"
                    ? "Downloading model weights"
                    : "Downloading tokenizer"}
                </span>
                <span>{formatProgress(llmProgress)}</span>
              </div>
              <div
                style={{
                  height: 6,
                  borderRadius: 3,
                  background: "rgba(255,255,255,0.06)",
                  overflow: "hidden",
                  position: "relative",
                }}
              >
                {llmProgress.bytesTotal > 0 ? (
                  <div
                    style={{
                      width: `${Math.min(
                        100,
                        (llmProgress.bytesDownloaded / llmProgress.bytesTotal) * 100,
                      ).toFixed(2)}%`,
                      height: "100%",
                      background: "var(--blue, rgb(120,160,255))",
                      transition: "width 200ms linear",
                    }}
                  />
                ) : (
                  <div
                    style={{
                      width: "30%",
                      height: "100%",
                      background: "var(--blue, rgb(120,160,255))",
                      animation: "slide-indeterminate 1.5s infinite linear",
                    }}
                  />
                )}
              </div>
            </div>
          ) : null}

          {llmDiagnostic ? (
            <div
              style={{
                marginTop: 12,
                padding: "12px 14px",
                borderRadius: 10,
                background: llmDiagnostic.ok
                  ? "rgba(80,170,110,0.06)"
                  : "rgba(220,170,90,0.06)",
                border: `1px solid ${
                  llmDiagnostic.ok
                    ? "rgba(80,170,110,0.22)"
                    : "rgba(220,170,90,0.22)"
                }`,
                fontSize: 12,
                lineHeight: 1.5,
                maxWidth: 720,
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  color: llmDiagnostic.ok
                    ? "rgb(122,200,140)"
                    : "rgb(220,170,90)",
                  marginBottom: 8,
                  fontWeight: 600,
                }}
              >
                {llmDiagnostic.ok ? (
                  <CheckCircle size={12} />
                ) : (
                  <AlertCircle size={12} />
                )}
                {llmDiagnostic.message}
              </div>
              {llmDiagnostic.ok ? (
                <>
                  <div style={{ color: "var(--t-3)", marginBottom: 4 }}>
                    Prompt: {llmDiagnostic.prompt}
                  </div>
                  <div
                    style={{
                      color: "var(--text-primary)",
                      whiteSpace: "pre-wrap",
                      background: "rgba(0,0,0,0.2)",
                      padding: "8px 10px",
                      borderRadius: 6,
                    }}
                  >
                    {llmDiagnostic.response || "(empty response)"}
                  </div>
                </>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}

      <div
        style={{
          paddingTop: 24,
          borderTop: "1px solid rgba(255,255,255,0.05)",
          marginTop: 24,
        }}
      >
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 14 }}>
          OCR
        </div>
        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <button
            className="btn-ghost"
            onClick={() => void handleRebuild()}
            disabled={!enabled || !ocrAvailable || busy === "rebuild"}
          >
            <RefreshCw size={13} /> {busy === "rebuild" ? "Queueing…" : "Run OCR rebuild"}
          </button>
          <button
            className="btn-ghost"
            onClick={() => void handleForceScrub()}
            disabled={!enabled || busy === "scrub"}
            title="Re-runs the auto-tagger (with v0.5.6 URL/UUID guards), re-flags Recall-self-capture screenshots, and re-extracts entities for every memory. Safe to click any time — fully idempotent."
          >
            <RefreshCw size={13} /> {busy === "scrub" ? "Scrubbing…" : "Re-scrub AI tags"}
          </button>
          {queue ? (
            <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
              Queue: {queue.ocrQueued} pending · {queue.ocrRunning} running
              {queue.ocrFailed > 0 ? ` · ${queue.ocrFailed} failed` : ""}
            </span>
          ) : null}
        </div>
        {notice.kind !== "idle" ? (
          <div
            style={{
              marginTop: 12,
              fontSize: 12,
              color:
                notice.kind === "error" ? "var(--text-danger)" : "var(--text-muted)",
              display: "flex",
              alignItems: "center",
              gap: 6,
            }}
          >
            {notice.kind === "error" ? (
              <AlertCircle size={12} />
            ) : (
              <CheckCircle size={12} />
            )}
            {notice.message}
          </div>
        ) : null}
      </div>

      <div
        style={{
          paddingTop: 24,
          borderTop: "1px solid rgba(255,255,255,0.05)",
          marginTop: 24,
        }}
      >
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 6 }}>
          Diagnostics
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-muted)",
            marginBottom: 12,
            maxWidth: 540,
            lineHeight: 1.5,
          }}
        >
          Copy a screenshot and click below. If we can read it, you'll see the
          dimensions; if we can't, the message tells you why. Useful when an
          image you copied doesn't turn into a memory automatically.
        </div>
        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <button
            className="btn-ghost"
            onClick={() => void handleDiagnose()}
            disabled={busy === "diagnose"}
          >
            <Clipboard size={13} />
            {busy === "diagnose" ? "Reading…" : "Test clipboard image"}
          </button>
        </div>
        {diagnostic ? (
          <div
            style={{
              marginTop: 12,
              fontSize: 12,
              color: diagnostic.ok ? "rgb(122,200,140)" : "var(--text-danger, #f87171)",
              display: "flex",
              alignItems: "flex-start",
              gap: 6,
              maxWidth: 600,
              lineHeight: 1.5,
            }}
          >
            {diagnostic.ok ? (
              <CheckCircle size={12} style={{ marginTop: 2, flexShrink: 0 }} />
            ) : (
              <AlertCircle size={12} style={{ marginTop: 2, flexShrink: 0 }} />
            )}
            <span>{diagnostic.message}</span>
          </div>
        ) : null}
      </div>
    </Section>
  );
}

/* ─── small local primitives so we don't fork the parent file ───── */

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h2
        style={{
          fontSize: 19,
          fontWeight: 700,
          color: "var(--text-primary)",
          letterSpacing: "-0.01em",
        }}
      >
        {title}
      </h2>
      <div className="accent-line" style={{ marginBottom: 24 }} />
      {children}
    </div>
  );
}

function Toggle({
  label,
  description,
  value,
  onChange,
  disabled,
  icon,
}: {
  label: string;
  description: string;
  value: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  icon?: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "16px 0",
        borderBottom: "1px solid rgba(255,255,255,0.05)",
        opacity: disabled ? 0.55 : 1,
      }}
    >
      <div>
        <div
          style={{
            fontSize: 14,
            fontWeight: 500,
            color: "var(--text-primary)",
            marginBottom: 2,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          {icon ? <span style={{ color: "var(--blue)" }}>{icon}</span> : null}
          {label}
        </div>
        <div style={{ fontSize: 13, color: "var(--text-muted)" }}>{description}</div>
      </div>
      <button
        className={`toggle ${value ? "on" : ""}`}
        onClick={() => !disabled && onChange(!value)}
        disabled={disabled}
        style={disabled ? { cursor: "not-allowed" } : undefined}
      >
        <div className="toggle-thumb" />
      </button>
    </div>
  );
}

function ReadoutRow({
  icon,
  label,
  value,
  warn,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  warn?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "16px 0",
        borderBottom: "1px solid rgba(255,255,255,0.05)",
      }}
    >
      <div
        style={{
          fontSize: 14,
          color: "var(--text-primary)",
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        <span style={{ color: "var(--blue)" }}>{icon}</span>
        {label}
      </div>
      <div
        style={{
          fontSize: 13,
          color: warn ? "var(--text-danger, #f87171)" : "var(--text-muted)",
          textAlign: "right",
        }}
      >
        {value}
      </div>
    </div>
  );
}

/// v0.5.21: row with a label + description on the left and a
/// `<select>` dropdown on the right. Same layout language as
/// `Toggle` / `ReadoutRow` so the new Performance subsection
/// doesn't visually drift from the rest of the tab.
function DropdownRow({
  icon,
  label,
  description,
  value,
  onChange,
  options,
  disabled,
}: {
  icon: React.ReactNode;
  label: string;
  description: string;
  value: string;
  onChange: (next: string) => void;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 16,
        padding: "16px 0",
        borderBottom: "1px solid rgba(255,255,255,0.05)",
        opacity: disabled ? 0.55 : 1,
      }}
    >
      <div style={{ minWidth: 0, flex: 1 }}>
        <div
          style={{
            fontSize: 14,
            fontWeight: 500,
            color: "var(--text-primary)",
            marginBottom: 2,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <span style={{ color: "var(--blue)" }}>{icon}</span>
          {label}
        </div>
        <div style={{ fontSize: 13, color: "var(--text-muted)", lineHeight: 1.45 }}>
          {description}
        </div>
      </div>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        style={{
          flexShrink: 0,
          minWidth: 180,
          padding: "8px 10px",
          borderRadius: 8,
          background: "var(--panel)",
          border: "1px solid rgba(255,255,255,0.08)",
          color: "var(--text-primary)",
          fontSize: 13,
          cursor: disabled ? "not-allowed" : "pointer",
        }}
      >
        {options.map((opt) => (
          // v0.5.22: native <option> elements in Tauri's WebView
          // don't inherit the parent <select>'s color styles —
          // the popup uses Windows' system chrome by default,
          // which renders white-on-white in dark mode. Explicit
          // inline `background` + `color` forces Chromium to
          // draw option chrome with readable contrast.
          <option
            key={opt.value}
            value={opt.value}
            style={{ background: "#1a1a1a", color: "#f0f0f0" }}
          >
            {opt.label}
          </option>
        ))}
      </select>
    </div>
  );
}
