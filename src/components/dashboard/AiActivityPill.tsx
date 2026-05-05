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
import { AlertCircle, Loader2, Pause, Sparkles } from "lucide-react";

import { aiClient } from "@/services/ai/AiClient";
import type { AiStatusPayload } from "@/domain/types";
import type { MainView } from "@/windows/MainWindow";

interface AiActivityPillProps {
  setView: (view: MainView) => void;
}

const POLL_INTERVAL_MS = 5000;

export function AiActivityPill({ setView }: AiActivityPillProps) {
  const [status, setStatus] = useState<AiStatusPayload | null>(null);

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

  const handleClick = useCallback(() => {
    setView("settings");
  }, [setView]);

  if (!status) return null;

  // Pick the highest-priority surfaceable state. Order matters:
  // failures and disabled states beat "things are working" copy
  // because the user needs to act on those.
  const variant = pickVariant(status);
  if (!variant) return null;

  return (
    <button
      type="button"
      onClick={handleClick}
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
      title="Open AI Settings"
    >
      {variant.icon}
      {variant.label}
    </button>
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

  const queue = status.queue;

  // 3. Failed jobs at max retries. Users care about this because
  // the data is broken — failed memories never get OCR'd unless
  // someone clicks rebuild.
  if (queue.failed > 0) {
    return {
      background: "rgba(245, 158, 11, 0.08)",
      borderColor: "rgba(245, 158, 11, 0.25)",
      textColor: "rgba(245, 158, 11, 0.95)",
      icon: <AlertCircle size={11} strokeWidth={2} />,
      label: `${queue.failed} OCR job${queue.failed === 1 ? "" : "s"} failed — open Settings`,
    };
  }

  // 4. Active processing. Neutral — just informational.
  if (queue.running > 0) {
    return {
      background: "rgba(99, 102, 241, 0.08)",
      borderColor: "rgba(99, 102, 241, 0.25)",
      textColor: "rgba(99, 102, 241, 0.95)",
      icon: <Loader2 size={11} strokeWidth={2} className="spin" />,
      label:
        queue.queued > 0
          ? `Processing ${queue.running} · ${queue.queued} queued`
          : `Processing ${queue.running} capture${queue.running === 1 ? "" : "s"}…`,
    };
  }

  // 5. Queue has work but worker isn't claiming. Could be a gate
  // (battery / pause-on-battery / heavy-only-on-AC / low-battery)
  // or a stuck worker. Either way the user wants to know.
  if (queue.queued > 0) {
    return {
      background: "rgba(245, 158, 11, 0.08)",
      borderColor: "rgba(245, 158, 11, 0.25)",
      textColor: "rgba(245, 158, 11, 0.95)",
      icon: <Pause size={11} strokeWidth={2} />,
      label: `${queue.queued} capture${queue.queued === 1 ? "" : "s"} waiting — paused?`,
    };
  }

  // 6. Happy idle path — hide the pill. Don't clutter Home with
  // "AI is fine" copy; absence of the pill is itself the signal.
  return null;
}
