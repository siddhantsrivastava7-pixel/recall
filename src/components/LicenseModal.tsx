import { useEffect, useMemo, useRef, useState } from "react";
import { KeyRound, Loader2 } from "lucide-react";

import { useLicenseStore } from "@/stores/licenseStore";

export function LicenseModal() {
  const inputRef = useRef<HTMLInputElement>(null);
  const [key, setKey] = useState("");
  const {
    status,
    isExpired,
    error,
    expiresAt,
    activateKey,
  } = useLicenseStore();

  useEffect(() => {
    const id = window.setTimeout(() => inputRef.current?.focus(), 60);
    return () => window.clearTimeout(id);
  }, []);

  const isValidating = status === "validating";
  const title = isExpired ? "Trial expired" : "Enter your trial key";
  const subtitle = isExpired
    ? "Your 7-day Recall trial has ended. Enter a new trial key to continue."
    : "Activate Recall once, then keep using it offline until the trial expires.";
  const statusText = useMemo(() => {
    if (status === "success" && expiresAt) {
      return `Trial activated successfully. Expires ${new Date(expiresAt).toLocaleDateString()}.`;
    }
    if (status === "invalid") return "Invalid key.";
    if (status === "expired") return "Trial expired.";
    if (status === "network-error") return error ?? "Network error.";
    return error;
  }, [error, expiresAt, status]);

  async function submit() {
    if (isValidating) return;
    await activateKey(key);
  }

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 32,
        background: "linear-gradient(135deg, #0B0F1A 0%, #0E1424 60%, #0B1020 100%)",
        position: "relative",
        overflow: "hidden",
      }}
    >
      <div className="recall-noise" />
      <div
        style={{
          position: "absolute",
          width: 520,
          height: 520,
          borderRadius: "50%",
          background: "radial-gradient(circle, rgba(79,124,255,0.12) 0%, transparent 68%)",
          top: -120,
          right: -40,
          pointerEvents: "none",
        }}
      />

      <section
        className="anim-scalein"
        style={{
          width: "min(480px, 100%)",
          borderRadius: 28,
          border: "1px solid rgba(255,255,255,0.10)",
          background: "linear-gradient(180deg, rgba(17,24,39,0.94) 0%, rgba(12,18,32,0.92) 100%)",
          boxShadow: "0 24px 80px rgba(0,0,0,0.46)",
          padding: 30,
          position: "relative",
          zIndex: 1,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 22 }}>
          <div
            style={{
              width: 44,
              height: 44,
              borderRadius: "50%",
              background: "var(--blue-dim)",
              border: "1px solid var(--blue-border)",
              color: "var(--blue)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <KeyRound size={19} />
          </div>
          <div>
            <div className="eyebrow" style={{ marginBottom: 5 }}>Recall Trial</div>
            <h1 style={{ fontSize: 24, lineHeight: 1.12, letterSpacing: "-0.03em", color: "var(--text-primary)" }}>
              {title}
            </h1>
          </div>
        </div>

        <p style={{ color: "var(--text-muted)", fontSize: 14, lineHeight: 1.7, marginBottom: 22 }}>
          {subtitle}
        </p>

        <div style={{ display: "flex", gap: 10 }}>
          <input
            ref={inputRef}
            value={key}
            onChange={(event) => setKey(event.target.value.toUpperCase())}
            onPaste={() => {
              window.setTimeout(() => setKey((value) => value.trim().toUpperCase()), 0);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void submit();
              }
            }}
            placeholder="RC-TRIAL-XXXX-XXXX"
            className="r-input"
            style={{
              height: 46,
              fontFamily: "monospace",
              letterSpacing: "0.05em",
              flex: 1,
            }}
          />
          <button
            className="btn-primary"
            disabled={isValidating || !key.trim()}
            onClick={() => void submit()}
            style={{ height: 46 }}
          >
            {isValidating && <Loader2 size={14} className="animate-spin" />}
            {isValidating ? "Activating..." : "Activate"}
          </button>
        </div>

        {statusText && (
          <div
            style={{
              marginTop: 14,
              fontSize: 13,
              color: status === "success" ? "var(--success)" : "var(--danger)",
            }}
          >
            {statusText}
          </div>
        )}

        <div style={{ marginTop: 22, paddingTop: 18, borderTop: "1px solid rgba(255,255,255,0.06)", fontSize: 12, color: "rgba(255,255,255,0.34)", lineHeight: 1.6 }}>
          Trial validation happens once online. After activation, your license state is stored locally and Recall works offline until expiry.
        </div>
      </section>
    </div>
  );
}
