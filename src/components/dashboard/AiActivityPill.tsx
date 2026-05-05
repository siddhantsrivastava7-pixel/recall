/**
 * v0.5.28 — AI activity pill.
 *
 * One-line status indicator on Home that surfaces what the AI
 * subsystem is doing right now. The motivating bug: a user with
 * AI enabled, on AC power, and Windows.Media.Ocr installed reported
 * fresh screenshots stuck pre-OCR for minutes. There was no way
 * for them to tell whether the worker was running, gated, or
 * silently failing without digging into Settings.
 *
 * Visibility model — show ONLY when there's something worth
 * surfacing. The pill stays hidden in the happy path (AI on,
 * queue empty). It appears when:
 *
 *   * AI is off          → "AI is off · open Settings"
 *   * OCR unsupported    → "OCR unavailable on this host"
 *   * Failed jobs        → "{N} OCR job(s) failed · open Settings"
 *   * Active processing  → "Processing {N} captures…"
 *   * Queued, no running → "{N} captures waiting"
 *
 * Clicking the pill jumps to Settings → AI tab so the user can
 * see the full readout (engine, queue counts, toggles) and act.
 *
 * Polls `ai_status` every 5s while mounted. Cheap — the underlying
 * query is a few COUNT aggregates over `ai_work_queue`. The poll
 * loop disposes cleanly on unmount and on view-switch.
 */

import { useCallback, useEffect, useState } from "react";
import { AlertCircle, ArrowRight, Loader2, Pause, X } from "lucide-react";

import { aiClient, type AiFailedJob } from "@/services/ai/AiClient";
import { useMemoryStore } from "@/stores/memoryStore";
import type { AiStatusPayload } from "@/domain/types";
import type { MainView } from "@/windows/MainWindow";

interface AiActivityPillProps {
  setView: (view: MainView) => void;
}

const POLL_INTERVAL_MS = 5000;

export function AiActivityPill({ setView }: AiActivityPillProps) {
  const [status, setStatus] = useState<AiStatusPayload | null>(null);
  const selectMemory = useMemoryStore((state) => state.selectMemory);
  // v0.5.29: when the failed-jobs modal opens, we fetch the most
  // recent failures and render their `lastError` strings inline so
  // the user can see the actual cause (file missing / unsupported
  // format / engine error / etc.).
  const [showFailures, setShowFailures] = useState(false);
  const [failures, setFailures] = useState<AiFailedJob[]>([]);
  const [failuresLoading, setFailuresLoading] = useState(false);
  // v0.5.30: clear-failed-jobs action state. We track in-flight
  // separately so the button can disable itself + show a "Clearing…"
  // label without disabling the rest of the modal.
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    let disposed = false;

    const fetchOnce = async () => {
      try {
        const result = await aiClient.status();
        if (!disposed) setStatus(result);
      } catch {
        // ai_status is supposed to be infallible (returns errors as
        // payload fields, not as rejections), but a panic in the
        // backend would surface as a rejection here. Hide the pill
        // rather than render something half-broken.
        if (!disposed) setStatus(null);
      }
    };

    void fetchOnce();
    const id = setInterval(() => void fetchOnce(), POLL_INTERVAL_MS);

    return () => {
      disposed = true;
      clearInterval(id);
    };
  }, []);

  // Modal opens on click WHEN we have failed jobs to show.
  // Otherwise the click jumps to Settings (no failures = the
  // user is here for a different reason — gates, AI off, etc.).
  const failedCount = status
    ? status.queue.ocrFailed + status.queue.embedFailed
    : 0;

  const handleClick = useCallback(async () => {
    if (failedCount > 0) {
      setShowFailures(true);
      setFailuresLoading(true);
      try {
        const rows = await aiClient.recentAiFailures();
        setFailures(rows);
      } catch (error) {
        console.error("[recall] recent failures fetch failed:", error);
        setFailures([]);
      } finally {
        setFailuresLoading(false);
      }
      return;
    }
    setView("settings");
  }, [failedCount, setView]);

  const handleCloseModal = useCallback(() => {
    setShowFailures(false);
  }, []);

  // v0.5.31: clicking a failure row jumps to the affected
  // memory's detail view. Lets the user see what the orphan
  // actually is (legacy capture, manual test row, etc.) before
  // deciding to clear it.
  const handleViewMemory = useCallback(
    (memoryId: string) => {
      selectMemory(memoryId);
      setView("memories");
      setShowFailures(false);
    },
    [selectMemory, setView],
  );

  const handleClearFailed = useCallback(async () => {
    if (clearing) return;
    setClearing(true);
    try {
      await aiClient.clearFailedOcr();
      // Refresh both views: the modal list (now empty) and the
      // pill itself (so the count drops to 0 immediately, which
      // hides the pill on the happy-idle path).
      setFailures([]);
      const fresh = await aiClient.status();
      setStatus(fresh);
      // Auto-close the modal when the queue is now clean — the
      // user just acted on it; no point staring at an empty list.
      setShowFailures(false);
    } catch (error) {
      console.error("[recall] clear failed jobs failed:", error);
    } finally {
      setClearing(false);
    }
  }, [clearing]);

  if (!status) return null;

  // Pick the highest-priority surfaceable state. Order matters:
  // failures and disabled states beat "things are working" copy
  // because the user needs to act on those.
  const variant = pickVariant(status);
  if (!variant) return null;

  return (
    <>
      <button
        type="button"
        onClick={() => void handleClick()}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 8,
          padding: "6px 12px",
          borderRadius: 999,
          background: variant.background,
          border: `1px solid ${variant.borderColor}`,
          color: variant.textColor,
          fontSize: 12,
          fontWeight: 500,
          cursor: "pointer",
          // Inline so this can be slotted into a flex row without
          // extra wrapping containers.
          marginTop: 14,
        }}
        title={
          failedCount > 0
            ? "Click to see why these failed"
            : "Open AI Settings"
        }
      >
        {variant.icon}
        {variant.label}
      </button>

      {/*
        v0.5.29 — failure-error modal. Fixed position over the
        page when open. Renders the actual `lastError` string from
        each failed job row so the user can see what's actually
        broken instead of just seeing "3 jobs failed."
      */}
      {showFailures ? (
        <div
          onClick={handleCloseModal}
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.55)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 100,
            padding: 20,
          }}
        >
          <div
            onClick={(event) => event.stopPropagation()}
            style={{
              maxWidth: 720,
              width: "100%",
              maxHeight: "80vh",
              overflowY: "auto",
              background: "var(--panel)",
              border: "1px solid var(--border-default)",
              borderRadius: 14,
              padding: 22,
              color: "var(--text-primary)",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                marginBottom: 14,
              }}
            >
              <div>
                <div
                  style={{
                    fontSize: 11,
                    fontWeight: 650,
                    letterSpacing: "0.12em",
                    textTransform: "uppercase",
                    color: "rgba(245, 158, 11, 0.95)",
                    marginBottom: 6,
                  }}
                >
                  Recent AI failures
                </div>
                <div
                  style={{
                    fontSize: 18,
                    fontWeight: 600,
                    color: "var(--text-primary)",
                  }}
                >
                  {failedCount} job{failedCount === 1 ? "" : "s"} failed
                </div>
              </div>
              <button
                type="button"
                aria-label="Close"
                onClick={handleCloseModal}
                style={{
                  background: "transparent",
                  border: "none",
                  color: "var(--t-3)",
                  cursor: "pointer",
                  padding: 6,
                  borderRadius: 6,
                }}
              >
                <X size={16} strokeWidth={1.8} />
              </button>
            </div>

            {failuresLoading ? (
              <div style={{ fontSize: 13, color: "var(--t-3)" }}>
                Loading failure details…
              </div>
            ) : failures.length === 0 ? (
              <div style={{ fontSize: 13, color: "var(--t-3)" }}>
                No failure details available.
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                {failures.map((failure) => (
                  <FailureRow
                    key={failure.id}
                    failure={failure}
                    onViewMemory={handleViewMemory}
                  />
                ))}
              </div>
            )}

            <div
              style={{
                marginTop: 18,
                display: "flex",
                gap: 12,
                alignItems: "center",
                flexWrap: "wrap",
              }}
            >
              {/*
                v0.5.30 — primary action. Clears every dead-lettered
                OCR row from the queue. The most common cause is
                orphan screenshots (memory rows whose backing image
                file got purged), which can never be re-OCR'd.
                Clearing the count is the right action; the
                memories themselves stay and remain searchable.
              */}
              <button
                type="button"
                onClick={() => void handleClearFailed()}
                disabled={clearing || failures.length === 0}
                style={{
                  padding: "8px 14px",
                  borderRadius: 10,
                  background: "rgba(244, 63, 94, 0.12)",
                  border: "1px solid rgba(244, 63, 94, 0.30)",
                  color: "rgba(244, 63, 94, 0.95)",
                  fontSize: 12,
                  fontWeight: 500,
                  cursor: clearing ? "not-allowed" : "pointer",
                  opacity: failures.length === 0 ? 0.55 : 1,
                }}
              >
                {clearing ? "Clearing…" : "Clear failed jobs"}
              </button>
              <div
                style={{
                  fontSize: 11,
                  color: "var(--t-4)",
                  lineHeight: 1.5,
                  flex: 1,
                  minWidth: 240,
                }}
              >
                Removes failed jobs from the queue. The memories
                themselves stay — you can still find them in your
                library. Use Settings → AI →{" "}
                <strong>Run OCR rebuild</strong> if you want to
                retry transient failures.
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

function FailureRow({
  failure,
  onViewMemory,
}: {
  failure: AiFailedJob;
  onViewMemory: (memoryId: string) => void;
}) {
  // Parse memory_id out of the dedupe_key for OCR jobs so we can
  // hint at which capture failed. dedupe_key shape:
  // `ocr:<memory_id>:<engine>` → split on `:` and take index 1.
  const memoryId =
    failure.kind === "ocr" && failure.dedupeKey.startsWith("ocr:")
      ? failure.dedupeKey.split(":")[1] ?? null
      : null;
  return (
    <div
      style={{
        padding: 12,
        borderRadius: 10,
        background: "var(--surface-2, rgba(255,255,255,0.03))",
        border: "1px solid rgba(255,255,255,0.06)",
      }}
    >
      <div
        style={{
          display: "flex",
          gap: 8,
          alignItems: "baseline",
          marginBottom: 6,
        }}
      >
        <span
          style={{
            fontSize: 10,
            fontWeight: 650,
            letterSpacing: "0.12em",
            textTransform: "uppercase",
            color: "var(--t-4)",
          }}
        >
          {failure.kind === "ocr" ? "OCR" : failure.kind}
        </span>
        <span style={{ fontSize: 11, color: "var(--t-4)" }}>
          {failure.attempts} attempt{failure.attempts === 1 ? "" : "s"}
        </span>
        {memoryId ? (
          <span
            style={{
              fontSize: 11,
              color: "var(--t-4)",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            }}
          >
            · {memoryId.slice(0, 8)}
          </span>
        ) : null}
      </div>
      <div
        style={{
          fontSize: 13,
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          color: "var(--text-primary)",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          userSelect: "text",
          WebkitUserSelect: "text",
          marginBottom: memoryId ? 8 : 0,
        }}
      >
        {failure.lastError ?? "(no error message recorded)"}
      </div>
      {/*
        v0.5.31: link to the memory whose OCR job failed. Critical
        for diagnosing what KIND of orphan we're dealing with — a
        legacy capture, a manual test row, or something else
        entirely. Click closes the modal and navigates to detail.
      */}
      {memoryId ? (
        <button
          type="button"
          onClick={() => onViewMemory(memoryId)}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "5px 10px",
            borderRadius: 8,
            background: "transparent",
            border: "1px solid rgba(255,255,255,0.10)",
            color: "var(--t-2)",
            fontSize: 11,
            cursor: "pointer",
          }}
          title="Open this memory to see what kind of orphan it is"
        >
          View memory
          <ArrowRight size={11} strokeWidth={1.9} />
        </button>
      ) : null}
    </div>
  );
}

interface Variant {
  background: string;
  borderColor: string;
  textColor: string;
  icon: React.ReactNode;
  label: string;
}

function pickVariant(status: AiStatusPayload): Variant | null {
  // 1. AI master toggle off — highest-priority alert because
  // every downstream feature is gated on it.
  if (!status.enabled) {
    return {
      background: "rgba(244, 63, 94, 0.08)",
      borderColor: "rgba(244, 63, 94, 0.25)",
      textColor: "rgba(244, 63, 94, 0.95)",
      icon: <Pause size={11} strokeWidth={2} />,
      label: "AI is off — open Settings to enable",
    };
  }

  // 2. OCR engine reports unsupported. On Windows this typically
  // means the language pack isn't installed; on macOS it's a
  // stale framework. User can't fix without docs, but at least
  // they know why screenshots aren't being processed.
  if (!status.ocrAvailable) {
    return {
      background: "rgba(245, 158, 11, 0.08)",
      borderColor: "rgba(245, 158, 11, 0.25)",
      textColor: "rgba(245, 158, 11, 0.95)",
      icon: <AlertCircle size={11} strokeWidth={2} />,
      label: "OCR unavailable on this host",
    };
  }

  // SchedulerStatus is split per-kind (OCR vs embed). Today we
  // collapse them for the pill — the user just wants to know
  // "is the AI worker stuck?" not which kind. Sum the counts;
  // labels still emphasize OCR because that's the dominant case
  // (every screenshot enqueues OCR; embeds are throttled separately).
  const totalQueued = status.queue.ocrQueued + status.queue.embedQueued;
  const totalRunning = status.queue.ocrRunning + status.queue.embedRunning;
  const totalFailed = status.queue.ocrFailed + status.queue.embedFailed;

  // 3. Failed jobs at max retries. Users care about this because
  // the data is broken — failed memories never get OCR'd unless
  // someone clicks rebuild.
  if (totalFailed > 0) {
    return {
      background: "rgba(245, 158, 11, 0.08)",
      borderColor: "rgba(245, 158, 11, 0.25)",
      textColor: "rgba(245, 158, 11, 0.95)",
      icon: <AlertCircle size={11} strokeWidth={2} />,
      label: `${totalFailed} AI job${totalFailed === 1 ? "" : "s"} failed — open Settings`,
    };
  }

  // 4. Active processing. Neutral — just informational.
  if (totalRunning > 0) {
    return {
      background: "rgba(99, 102, 241, 0.08)",
      borderColor: "rgba(99, 102, 241, 0.25)",
      textColor: "rgba(99, 102, 241, 0.95)",
      icon: <Loader2 size={11} strokeWidth={2} className="spin" />,
      label:
        totalQueued > 0
          ? `Processing ${totalRunning} · ${totalQueued} queued`
          : `Processing ${totalRunning} capture${totalRunning === 1 ? "" : "s"}…`,
    };
  }

  // 5. Queue has work but worker isn't claiming. Could be a gate
  // (battery / pause-on-battery / heavy-only-on-AC / low-battery)
  // or a stuck worker. Either way the user wants to know.
  if (totalQueued > 0) {
    return {
      background: "rgba(245, 158, 11, 0.08)",
      borderColor: "rgba(245, 158, 11, 0.25)",
      textColor: "rgba(245, 158, 11, 0.95)",
      icon: <Pause size={11} strokeWidth={2} />,
      label: `${totalQueued} capture${totalQueued === 1 ? "" : "s"} waiting — paused?`,
    };
  }

  // 6. Happy idle path — hide the pill. Don't clutter Home with
  // "AI is fine" copy; absence of the pill is itself the signal.
  return null;
}
