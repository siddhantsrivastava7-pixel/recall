/**
 * v0.5.23 — Proactive surface slot for Home.
 *
 * Renders ONE card at the top of the dashboard. The backend's
 * surface engine decides which kind wins (Weekly recap vs.
 * Forgotten Gold) — this component is a thin renderer plus
 * dismiss action. When the engine returns null (no qualifying
 * surface), the slot stays hidden — no empty placeholder, no
 * dashboard syndrome.
 *
 * Strict product rule (locked v0.5.23): one surface at a time.
 * If we ever want to stack surfaces, that's a deliberate later
 * decision, not something this component drifts into.
 *
 * The card primitive is intentionally minimal: small "kind"
 * eyebrow, headline = memory title, subtitle = reason, and two
 * actions (Open + Dismiss). Variants are styling differences
 * (icon, accent color) keyed by kind so adding new kinds in
 * v0.5.24+ requires zero new components.
 */

import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Sparkles, Clock3, X, BookmarkCheck, GitBranch } from "lucide-react";

import { aiClient, type ActiveProactiveSurface } from "@/services/ai/AiClient";
import { useMemoryStore } from "@/stores/memoryStore";
import type { MainView } from "@/windows/MainWindow";

interface ProactiveSurfaceProps {
  setView: (view: MainView) => void;
}

export function ProactiveSurface({ setView }: ProactiveSurfaceProps) {
  const [active, setActive] = useState<ActiveProactiveSurface | null>(null);
  const [loaded, setLoaded] = useState(false);
  const selectMemory = useMemoryStore((state) => state.selectMemory);
  const upsertMemory = useMemoryStore((state) => state.upsertMemory);

  // Fetch on mount. The backend caches per day for Forgotten Gold
  // and per week for Weekly recap, so re-fetching on every Home
  // mount is cheap (one SELECT + maybe one row INSERT on the
  // first call of the day/week).
  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        const result = await aiClient.getProactiveSurface();
        if (!disposed) {
          // v0.5.26 fix — push the surface's memory into the
          // memory store so MemoriesView's `find(id)` lookup
          // resolves when "Open recap" navigates over. Without
          // this, the recap memory exists in SQLite but not in
          // the frontend store — the bookkeeping side-effect of
          // the engine creating a memory through `repo.create`
          // (which goes around `capture_service` and its
          // post-save event) leaves the store unaware. The user
          // saw "Open recap" → All Memories list with no detail
          // panel because the memory was found in neither.
          if (result) {
            upsertMemory(result.memory);
          }
          setActive(result ?? null);
          setLoaded(true);
        }
      } catch (error) {
        // Surfaces are non-critical. A failure here should never
        // block Home from rendering — just hide the slot.
        console.error("[recall] proactive surface fetch failed:", error);
        if (!disposed) {
          setLoaded(true);
        }
      }
    })();
    return () => {
      disposed = true;
    };
  }, [upsertMemory]);

  const handleOpen = useCallback(() => {
    if (!active) return;
    selectMemory(active.memory.id);
    setView("memories");
  }, [active, selectMemory, setView]);

  const handleDismiss = useCallback(async () => {
    if (!active) return;
    // Optimistic hide — swap to null immediately so the user sees
    // the slot collapse on click, then send the backend update.
    // If the dismiss call fails the backend stays in sync on next
    // mount; we don't restore a card the user just dismissed.
    setActive(null);
    try {
      await aiClient.dismissProactiveSurface(active.surface.id);
    } catch (error) {
      console.error("[recall] dismiss failed:", error);
    }
  }, [active]);

  if (!loaded || !active) {
    return null;
  }

  const variant = variantFor(active.surface.kind);
  return (
    <section
      style={{
        marginBottom: 28,
        padding: "20px 24px",
        borderRadius: 16,
        background: variant.background,
        border: `1px solid ${variant.borderColor}`,
        position: "relative",
      }}
    >
      <button
        type="button"
        aria-label="Dismiss"
        onClick={() => void handleDismiss()}
        style={{
          position: "absolute",
          top: 14,
          right: 14,
          background: "transparent",
          border: "none",
          color: "var(--t-4)",
          cursor: "pointer",
          padding: 6,
          borderRadius: 6,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
        }}
        title="Dismiss"
      >
        <X size={14} strokeWidth={1.8} />
      </button>

      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 8,
          fontSize: 11,
          fontWeight: 650,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color: variant.eyebrowColor,
          marginBottom: 10,
        }}
      >
        {variant.icon}
        {variant.eyebrow}
      </div>

      <h2
        style={{
          fontSize: 22,
          fontWeight: 600,
          color: "var(--text-primary)",
          letterSpacing: "-0.01em",
          lineHeight: 1.25,
          marginBottom: 8,
          paddingRight: 32, // leave space for the dismiss button
          // Selectable so the user can copy the title quickly.
          userSelect: "text",
          WebkitUserSelect: "text",
        }}
      >
        {displayTitle(active)}
      </h2>

      {active.surface.reason ? (
        <p
          style={{
            fontSize: 14,
            lineHeight: 1.5,
            color: "var(--t-3)",
            marginBottom: 16,
            maxWidth: 640,
          }}
        >
          {active.surface.reason}
        </p>
      ) : null}

      <button
        type="button"
        onClick={handleOpen}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 8,
          padding: "10px 14px",
          borderRadius: 10,
          background: variant.actionBackground,
          color: variant.actionColor,
          border: "none",
          cursor: "pointer",
          fontSize: 13,
          fontWeight: 500,
        }}
      >
        {variant.actionLabel}
        <ArrowRight size={13} strokeWidth={1.9} />
      </button>
    </section>
  );
}

/// Pick a readable headline for the card. Prefer the memory's
/// title; fall back to first non-empty content line.
function displayTitle(active: ActiveProactiveSurface): string {
  const title = active.memory.title?.trim();
  if (title) return title;
  const firstLine = active.memory.content
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  return firstLine ?? "Untitled memory";
}

interface CardVariant {
  eyebrow: string;
  eyebrowColor: string;
  icon: React.ReactNode;
  background: string;
  borderColor: string;
  actionBackground: string;
  actionColor: string;
  actionLabel: string;
}

/// Map a surface kind to its visual variant. New kinds added in
/// v0.5.24+ register themselves here — the rest of the component
/// is kind-agnostic.
function variantFor(kind: string): CardVariant {
  switch (kind) {
    case "weekly_recap":
      return {
        eyebrow: "Weekly recap",
        eyebrowColor: "var(--accent, #6699ff)",
        icon: <Sparkles size={11} strokeWidth={1.9} />,
        background:
          "linear-gradient(135deg, rgba(102,153,255,0.10), rgba(102,153,255,0.04))",
        borderColor: "rgba(102,153,255,0.22)",
        actionBackground: "rgba(102,153,255,0.18)",
        actionColor: "var(--accent, #6699ff)",
        actionLabel: "Open recap",
      };
    case "forgotten_gold":
      return {
        eyebrow: "Forgotten gold",
        eyebrowColor: "rgba(212,175,55,0.95)",
        icon: <BookmarkCheck size={11} strokeWidth={1.9} />,
        background:
          "linear-gradient(135deg, rgba(212,175,55,0.10), rgba(212,175,55,0.04))",
        borderColor: "rgba(212,175,55,0.20)",
        actionBackground: "rgba(212,175,55,0.18)",
        actionColor: "rgba(212,175,55,0.95)",
        actionLabel: "Revisit memory",
      };
    case "active_thread":
      // v0.5.59 — Active Thread variant. Distinct visual key
      // from Weekly recap (blue, calm) and Forgotten gold
      // (amber, archival): a soft teal that says "current
      // momentum." Action label leads to the memory's detail
      // view, where the v0.5.58 Memory Trail renders the chain.
      return {
        eyebrow: "Active thread",
        eyebrowColor: "rgba(120,200,180,0.95)",
        icon: <GitBranch size={11} strokeWidth={1.9} />,
        background:
          "linear-gradient(135deg, rgba(120,200,180,0.10), rgba(120,200,180,0.04))",
        borderColor: "rgba(120,200,180,0.22)",
        actionBackground: "rgba(120,200,180,0.18)",
        actionColor: "rgba(120,200,180,0.95)",
        actionLabel: "View thread",
      };
    default:
      return {
        eyebrow: kind.replace(/_/g, " "),
        eyebrowColor: "var(--t-3)",
        icon: <Clock3 size={11} strokeWidth={1.9} />,
        background: "var(--panel)",
        borderColor: "var(--border-default)",
        actionBackground: "var(--panel)",
        actionColor: "var(--text-primary)",
        actionLabel: "Open",
      };
  }
}
