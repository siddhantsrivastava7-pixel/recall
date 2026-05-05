// Inline preview of the on-disk screenshot for memories captured from
// the clipboard image branch. Resolves the `file://...` URL via Tauri's
// asset protocol scope (configured in `tauri.conf.json`) so the
// webview can load the bytes directly without a Rust round-trip.
//
// Status pill mirrors `ocr_status`:
//   * `running` — "Reading text…" with subtle pulse
//   * `done`    — green check + "Searchable"
//   * `failed`  — amber dot + the error truncated
//   * everything else (null / pending) — grey dot + "Queued"
//
// The image itself stays the source of truth even when OCR fails;
// failed OCR is non-fatal — the user still has the screenshot.

import { convertFileSrc } from "@tauri-apps/api/core";
import { useMemo } from "react";
import { AlertTriangle, Archive, CheckCircle, Clock3, Eye } from "lucide-react";
import type { Memory } from "@/domain/types";

const SCREENSHOT_SOURCE = "screenshot";

/// True for any memory that originated as a screenshot — whether or
/// not the backing image file is still on disk. Pre-v0.5.32 this
/// also required a non-null `url`, which meant a purged screenshot
/// (60+ days old, file deleted by retention GC) silently dropped its
/// preview section entirely. Now the section ALWAYS renders for
/// screenshot memories; the component itself decides whether to show
/// the image, the OCR-only state, or a "purged" placeholder.
export function isScreenshotMemory(memory: Memory): boolean {
  return memory.sourceApp === SCREENSHOT_SOURCE;
}

function fileUrlToPath(url: string): string | null {
  if (!url.startsWith("file://")) return null;
  const stripped = url.slice("file://".length);
  // Windows file URLs come through as `file:///C:/...` — convertFileSrc
  // expects the path *without* the leading slash before the drive letter.
  if (/^\/[A-Za-z]:/.test(stripped)) {
    return stripped.slice(1);
  }
  return stripped;
}

export function ScreenshotPreview({ memory }: { memory: Memory }) {
  const src = useMemo(() => {
    if (!memory.url) return null;
    const path = fileUrlToPath(memory.url);
    if (!path) return null;
    try {
      return convertFileSrc(path);
    } catch {
      return null;
    }
  }, [memory.url]);

  // v0.5.32 — purged-image placeholder. When `url` is null on a
  // screenshot memory, the file has been removed (retention GC) but
  // the OCR text + content + title are still on the row. Show an
  // explicit panel saying so, instead of silently rendering nothing,
  // so the user understands the body below IS the screenshot's
  // content (extracted text), not a corrupted record.
  if (!src) {
    const hasOcrText = Boolean(memory.ocrText && memory.ocrText.trim().length > 0);
    return (
      <div style={{ marginBottom: 22 }}>
        <div
          style={{
            borderRadius: 12,
            border: "1px dashed rgba(255,255,255,0.10)",
            background: "rgba(255,255,255,0.02)",
            padding: "18px 20px",
            display: "flex",
            alignItems: "flex-start",
            gap: 14,
          }}
        >
          <div
            style={{
              width: 38,
              height: 38,
              borderRadius: 8,
              background: "rgba(255,255,255,0.04)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--t-3)",
              flexShrink: 0,
            }}
          >
            <Archive size={18} strokeWidth={1.7} />
          </div>
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                fontSize: 13,
                fontWeight: 600,
                color: "var(--text-primary)",
                marginBottom: 4,
              }}
            >
              Original screenshot was archived
            </div>
            <div
              style={{
                fontSize: 12,
                color: "var(--t-3)",
                lineHeight: 1.5,
              }}
            >
              {hasOcrText
                ? "The image file was removed by retention to save disk space, but the recognized text is preserved below — fully searchable. Disable retention in Settings → AI to keep image previews longer."
                : "The image file was removed by retention to save disk space, and OCR didn't capture any recognized text. The memory is still in your library. Disable retention in Settings → AI to keep image previews longer."}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ marginBottom: 22 }}>
      <div
        style={{
          borderRadius: 12,
          overflow: "hidden",
          border: "1px solid rgba(255,255,255,0.06)",
          background: "rgba(0,0,0,0.18)",
          maxWidth: "100%",
          display: "inline-block",
        }}
      >
        <img
          src={src}
          alt={memory.title ?? "Screenshot"}
          style={{
            display: "block",
            maxWidth: "100%",
            maxHeight: 540,
            height: "auto",
            objectFit: "contain",
          }}
        />
      </div>
      <OcrStatusPill memory={memory} />
    </div>
  );
}

function OcrStatusPill({ memory }: { memory: Memory }) {
  const status = memory.ocrStatus ?? null;
  let icon: React.ReactNode = <Clock3 size={12} strokeWidth={1.9} />;
  let label = "Queued";
  let color = "var(--t-3)";
  let background = "rgba(255,255,255,0.04)";
  let border = "rgba(255,255,255,0.06)";

  if (status === "running") {
    icon = <Eye size={12} strokeWidth={1.9} />;
    label = "Reading text…";
    color = "var(--blue)";
    background = "var(--blue-dim, rgba(64,128,255,0.12))";
    border = "var(--blue-border, rgba(64,128,255,0.28))";
  } else if (status === "done") {
    icon = <CheckCircle size={12} strokeWidth={1.9} />;
    if (memory.ocrText && memory.ocrText.trim().length > 0) {
      label = "Searchable";
    } else {
      label = "No text detected";
    }
    color = "rgb(122,200,140)";
    background = "rgba(80,170,110,0.10)";
    border = "rgba(80,170,110,0.28)";
  } else if (status === "failed") {
    icon = <AlertTriangle size={12} strokeWidth={1.9} />;
    label = memory.ocrError
      ? `OCR failed: ${truncate(memory.ocrError, 80)}`
      : "OCR failed";
    color = "rgb(220,170,90)";
    background = "rgba(220,170,90,0.08)";
    border = "rgba(220,170,90,0.26)";
  }

  return (
    <div
      style={{
        marginTop: 10,
        display: "inline-flex",
        alignItems: "center",
        gap: 7,
        padding: "5px 10px",
        borderRadius: 999,
        background,
        border: `1px solid ${border}`,
        color,
        fontSize: 12,
      }}
    >
      {icon}
      {label}
    </div>
  );
}

function truncate(value: string, max: number): string {
  if (value.length <= max) return value;
  return `${value.slice(0, max - 1)}…`;
}
