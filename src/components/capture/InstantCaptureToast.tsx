import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export function InstantCaptureToast() {
  const [visible, setVisible] = useState(false);
  const timeoutRef = useRef<number | null>(null);

  useEffect(() => {
    const unlistenPromise = listen("recall://instant-capture-saved", () => {
      if (timeoutRef.current) {
        window.clearTimeout(timeoutRef.current);
      }
      setVisible(true);
      timeoutRef.current = window.setTimeout(() => setVisible(false), 1400);
    });

    return () => {
      if (timeoutRef.current) {
        window.clearTimeout(timeoutRef.current);
      }
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  return (
    <div
      aria-live="polite"
      style={{
        position: "absolute",
        right: 28,
        bottom: 28,
        zIndex: 50,
        pointerEvents: "none",
        opacity: visible ? 1 : 0,
        transform: visible ? "translateY(0) scale(1)" : "translateY(8px) scale(0.98)",
        transition: "opacity 180ms ease, transform 180ms ease",
      }}
    >
      <div
        style={{
          padding: "9px 14px",
          borderRadius: 999,
          background: "rgba(17,24,39,0.88)",
          border: "1px solid rgba(79,124,255,0.24)",
          boxShadow: "0 18px 48px rgba(0,0,0,0.34)",
          backdropFilter: "blur(18px)",
          WebkitBackdropFilter: "blur(18px)",
          color: "rgba(229,231,235,0.92)",
          fontSize: 13,
          fontWeight: 600,
          letterSpacing: "-0.01em",
        }}
      >
        Saved ✓
      </div>
    </div>
  );
}
