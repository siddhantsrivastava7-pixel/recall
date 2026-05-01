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
} from "lucide-react";
import { aiClient, type ClipboardImageDiagnostic } from "@/services/ai/AiClient";
import { useSettingsStore } from "@/stores/settingsStore";
import type { AiStatusPayload } from "@/domain/types";

/// Poll cadence for the AI status snapshot while this tab is mounted.
/// 2s is fast enough that "Embed all memories" feels live without
/// hammering SQLite — the underlying query is a few COUNT aggregates.
const STATUS_POLL_INTERVAL_MS = 2000;

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
    "toggle" | "rebuild" | "diagnose" | "downloadModel" | "embedAll" | null
  >(null);
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
        const message =
          error instanceof Error ? error.message : "Unable to read AI status.";
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
      const message =
        error instanceof Error ? error.message : "Unable to toggle AI.";
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
      const message =
        error instanceof Error ? error.message : "Failed to start embedding pass.";
      setNotice({ kind: "error", message });
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
      const message =
        error instanceof Error ? error.message : "Failed to download embedding model.";
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
      const message =
        error instanceof Error ? error.message : "Unable to diagnose clipboard.";
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
      const message =
        error instanceof Error ? error.message : "Unable to rebuild OCR index.";
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
