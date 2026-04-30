import { useEffect, useState } from "react";
import { Zap, Keyboard, BookOpen, Key, CheckCircle, XCircle, RefreshCw, Download, Upload, Trash2, PackageCheck, Smartphone, Sparkles } from "lucide-react";
import { useSettingsStore } from "@/stores/settingsStore";
import { useAppStore } from "@/stores/appStore";
import { useUpdateStore } from "@/stores/updateStore";
import { useLicenseStore } from "@/stores/licenseStore";
import { usePairingStore } from "@/features/pairing/pairingStore";
import { tauriClient } from "@/services/api/tauri-client";
import { syncBookmarksNow } from "@/services/bookmarks";
import { getBookmarkBrowserOptions } from "@/domain/bookmarks";
import { formatLongTimestamp } from "@/domain/formatters";
import type { ShortcutBinding } from "@/domain/types";
import { AiSettingsTab } from "@/views/Settings/AiSettingsTab";

type Tab = "general" | "ai" | "shortcuts" | "bookmarks" | "pairing" | "updates" | "license";

export function SettingsView() {
  const [tab, setTab] = useState<Tab>("general");

  const TABS: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: "general",   label: "General",   icon: <Zap      size={14} /> },
    { id: "ai",        label: "AI",        icon: <Sparkles size={14} /> },
    { id: "shortcuts", label: "Shortcuts", icon: <Keyboard size={14} /> },
    { id: "bookmarks", label: "Bookmarks", icon: <BookOpen size={14} /> },
    { id: "pairing",   label: "Pairing",   icon: <Smartphone size={14} /> },
    { id: "updates",   label: "Updates",   icon: <PackageCheck size={14} /> },
    { id: "license",   label: "License",   icon: <Key      size={14} /> },
  ];

  return (
    <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
      {/* Settings nav */}
      <div style={{ width: 196, borderRight: "1px solid rgba(255,255,255,0.05)", padding: "36px 0 20px", flexShrink: 0 }}>
        <div className="eyebrow" style={{ padding: "0 18px", marginBottom: 14 }}>Settings</div>
        {TABS.map(t => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            style={{
              display: "flex", alignItems: "center", gap: 9,
              width: "100%", padding: "9px 18px",
              background: tab === t.id ? "var(--blue-dim)" : "none",
              color: tab === t.id ? "var(--text-primary)" : "var(--text-muted)",
              fontSize: 14, fontWeight: tab === t.id ? 500 : 400,
              cursor: "pointer", border: "none",
              borderLeft: `2px solid ${tab === t.id ? "var(--blue)" : "transparent"}`,
              fontFamily: "inherit", textAlign: "left",
              transition: "all 100ms",
            }}
          >
            <span style={{ color: tab === t.id ? "var(--blue)" : "var(--t-4)" }}>{t.icon}</span>
            {t.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflowY: "auto", padding: "36px 48px" }}>
        {tab === "general"   && <GeneralTab />}
        {tab === "ai"        && <AiSettingsTab />}
        {tab === "shortcuts" && <ShortcutsTab />}
        {tab === "bookmarks" && <BookmarksTab />}
        {tab === "pairing"   && <PairingTab />}
        {tab === "updates"   && <UpdatesTab />}
        {tab === "license"   && <LicenseTab />}
      </div>
    </div>
  );
}

/* ─── General ──────────────────────────────────────────────────── */
function GeneralTab() {
  const { settings, updateSettings } = useSettingsStore();

  return (
    <Section title="General">
      <Toggle
        label="Floating Widget"
        description="Show the floating pill on your desktop"
        value={settings.floatingWidgetEnabled}
        onChange={v => void updateSettings({ ...settings, floatingWidgetEnabled: v })}
      />
      <Toggle
        label="Launch on Startup"
        description="Start Recall automatically when you log in"
        value={settings.launchOnStartupEnabled}
        onChange={v => void updateSettings({ ...settings, launchOnStartupEnabled: v })}
      />

      <div style={{ paddingTop: 24, borderTop: "1px solid rgba(255,255,255,0.05)", marginTop: 8 }}>
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 14 }}>Data</div>
        <div style={{ display: "flex", gap: 10 }}>
          <button className="btn-ghost" onClick={() => void tauriClient.exportData()}>
            <Download size={13} /> Export Data
          </button>
          <button className="btn-ghost" onClick={() => void tauriClient.importData()}>
            <Upload size={13} /> Import Data
          </button>
          <button
            className="btn-danger"
            onClick={() => { if (confirm("Clear ALL data? This cannot be undone.")) void tauriClient.clearAllData(); }}
          >
            <Trash2 size={13} /> Clear All Data
          </button>
        </div>
      </div>
    </Section>
  );
}

/* ─── Shortcuts ─────────────────────────────────────────────────── */
function ShortcutsTab() {
  const { shortcuts, updateShortcuts } = useSettingsStore();
  const [editingAction, setEditingAction] = useState<string | null>(null);
  const [pendingShortcuts, setPendingShortcuts] = useState<ShortcutBinding[]>(shortcuts);
  const [message, setMessage] = useState("");

  useEffect(() => {
    setPendingShortcuts(shortcuts);
  }, [shortcuts]);

  async function saveShortcut(action: string, accelerator: string) {
    const nextShortcuts = pendingShortcuts.map((binding) =>
      binding.action === action ? { ...binding, accelerator } : binding,
    );
    setPendingShortcuts(nextShortcuts);
    const result = await updateShortcuts(nextShortcuts);
    if (!result.ok) {
      setMessage(result.error ?? "Unable to update shortcut.");
      setPendingShortcuts(shortcuts);
      return;
    }

    setMessage("Shortcut updated and applied.");
  }

  return (
    <Section title="Keyboard Shortcuts">
      {shortcuts.length === 0 ? (
        <div style={{ fontSize: 13, color: "var(--text-muted)" }}>No shortcuts configured.</div>
      ) : (
        <div>
          {pendingShortcuts.map((shortcut) => (
            <div
              key={shortcut.action}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 16,
                padding: "14px 0",
                borderBottom: "1px solid rgba(255,255,255,0.05)",
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 14, color: "var(--text-primary)", marginBottom: 3 }}>
                  {shortcut.description}
                </div>
                <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  {editingAction === shortcut.action
                    ? "Press a new shortcut combination"
                    : "Changes apply immediately if the shortcut is available."}
                </div>
              </div>

              <button
                onClick={() => {
                  setEditingAction(shortcut.action);
                  setMessage("");
                }}
                onKeyDown={(event) => {
                  if (editingAction !== shortcut.action) return;

                  if (event.key === "Escape") {
                    event.preventDefault();
                    setEditingAction(null);
                    return;
                  }

                  const accelerator = keyboardEventToAccelerator(event);
                  if (!accelerator) {
                    return;
                  }

                  event.preventDefault();
                  setEditingAction(null);
                  void saveShortcut(shortcut.action, accelerator);
                }}
                className="kbd"
                style={{
                  fontSize: 13,
                  minWidth: 148,
                  textAlign: "center",
                  cursor: "pointer",
                  color:
                    editingAction === shortcut.action
                      ? "var(--blue)"
                      : "var(--t-2)",
                  borderColor:
                    editingAction === shortcut.action
                      ? "var(--blue-border)"
                      : "rgba(255,255,255,0.10)",
                  background:
                    editingAction === shortcut.action
                      ? "var(--blue-dim)"
                      : "rgba(255,255,255,0.05)",
                }}
              >
                {editingAction === shortcut.action ? "Press keys..." : shortcut.accelerator}
              </button>
            </div>
          ))}
          {message && (
            <div style={{ marginTop: 12, fontSize: 13, color: "var(--text-muted)" }}>{message}</div>
          )}
        </div>
      )}
    </Section>
  );
}

/* ─── Bookmarks ─────────────────────────────────────────────────── */
function BookmarksTab() {
  const { settings, updateSettings } = useSettingsStore();
  const runtimePlatform = useAppStore((state) => state.runtime?.platform);
  const [syncing, setSyncing] = useState(false);
  const [syncMsg, setSyncMsg] = useState("");
  const browserOptions = getBookmarkBrowserOptions(runtimePlatform);

  async function syncNow() {
    setSyncing(true);
    setSyncMsg("");
    try {
      const res = await syncBookmarksNow();
      if (!res.ok || !res.data) {
        setSyncMsg(res.error ?? "Sync failed.");
      } else {
        setSyncMsg(`Imported ${res.data.totalImported}, skipped ${res.data.totalSkipped}.`);
      }
    } catch {
      setSyncMsg("Sync failed.");
    }
    setSyncing(false);
  }

  return (
    <Section title="Bookmark Sync">
      <Toggle
        label="Auto Sync"
        description="Automatically sync bookmarks in the background"
        value={settings.bookmarkAutoSyncEnabled}
        onChange={v => void updateSettings({ ...settings, bookmarkAutoSyncEnabled: v })}
      />

      <SettingRow label="Sync interval">
        <div style={{ display: "flex", gap: 7 }}>
          {[5, 15, 30, 60].map(mins => (
            <PillBtn
              key={mins}
              label={`${mins}m`}
              active={settings.bookmarkSyncIntervalMinutes === mins}
              onClick={() => void updateSettings({ ...settings, bookmarkSyncIntervalMinutes: mins })}
            />
          ))}
        </div>
      </SettingRow>

      <SettingRow label="Browsers">
        <div style={{ display: "flex", gap: 7 }}>
          {browserOptions.map(({ id, label }) => {
            const on = settings.bookmarkSyncBrowsers.includes(id);
            return (
              <PillBtn
                key={id}
                label={label}
                active={on}
                onClick={() => {
                  const list = on
                    ? settings.bookmarkSyncBrowsers.filter(x => x !== id)
                    : [...settings.bookmarkSyncBrowsers, id];
                  void updateSettings({ ...settings, bookmarkSyncBrowsers: list });
                }}
              />
            );
          })}
        </div>
      </SettingRow>

      <div style={{ paddingTop: 16 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <button className="btn-primary" onClick={syncNow} disabled={syncing}>
            <RefreshCw size={13} className={syncing ? "animate-spin" : ""} />
            {syncing ? "Syncing…" : "Sync Now"}
          </button>
          {syncMsg && <span style={{ fontSize: 13, color: "var(--text-muted)" }}>{syncMsg}</span>}
        </div>
        {settings.bookmarkLastSyncedAt && (
          <div style={{ marginTop: 10, fontSize: 12, color: "var(--t-4)", display: "flex", alignItems: "center", gap: 6 }}>
            <RefreshCw size={11} /> Last synced {formatLongTimestamp(settings.bookmarkLastSyncedAt)}
          </div>
        )}
      </div>
    </Section>
  );
}

/* Pairing */
function PairingTab() {
  const { info, loading, error, hydrate, reset } = usePairingStore();

  useEffect(() => {
    void hydrate();
  }, [hydrate]);

  return (
    <Section title="Mobile Pairing">
      <div
        style={{
          padding: "18px 22px",
          background: "var(--surface-2)",
          border: "1px solid var(--border-default)",
          borderRadius: 16,
          marginBottom: 18,
        }}
      >
        <div style={{ display: "flex", justifyContent: "space-between", gap: 18, alignItems: "flex-start" }}>
          <div>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
              {info?.receiverRunning ? "Ready to receive from phone" : "Receiver starting"}
            </div>
            <div style={{ marginTop: 6, fontSize: 13, color: "var(--text-muted)", lineHeight: 1.6 }}>
              Pairing is local-only. Your phone sends memories directly to this desktop over the same Wi-Fi.
            </div>
            <div style={{ marginTop: 12, fontSize: 12, color: "var(--t-3)", lineHeight: 1.8 }}>
              <div>Desktop: {info?.desktopName ?? "Loading..."}</div>
              <div>Endpoint: {info?.endpoint ?? "Waiting for local network..."}</div>
              <div>Device ID: {info?.deviceId ?? "Loading..."}</div>
            </div>
          </div>

          <button className="btn-ghost" onClick={() => void reset()} disabled={loading}>
            <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
            Reset pairing
          </button>
        </div>

        <div
          style={{
            marginTop: 18,
            padding: 14,
            borderRadius: 14,
            background: "rgba(255,255,255,0.035)",
            border: "1px solid rgba(255,255,255,0.06)",
          }}
        >
          <div style={{ fontSize: 12, color: "var(--t-3)", marginBottom: 8 }}>
            QR payload for mobile
          </div>
          <code
            style={{
              display: "block",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              fontSize: 12,
              lineHeight: 1.6,
              color: "var(--t-2)",
            }}
          >
            {info?.qrPayload ?? "Generating pairing payload..."}
          </code>
        </div>

        {error && (
          <div style={{ marginTop: 12, fontSize: 13, color: "var(--danger)" }}>
            {error}
          </div>
        )}
      </div>

      <div style={{ fontSize: 12, lineHeight: 1.7, color: "var(--t-3)" }}>
        The phone must call <code>GET /api/ping</code> or <code>POST /api/push-memory</code> with
        <code> Authorization: Bearer &lt;secret&gt;</code>. Reset pairing if a QR code was shared by mistake.
      </div>
    </Section>
  );
}

/* Updates */
function UpdatesTab() {
  const { settings, updateSettings } = useSettingsStore();
  const {
    currentVersion,
    status,
    checking,
    updateAvailable,
    availableVersion,
    releaseNotes,
    pubDate,
    downloading,
    downloadProgress,
    installing,
    lastCheckedAt,
    error,
    hydrateCurrentVersion,
    checkForUpdates,
    downloadAndInstallUpdate,
    resetError,
  } = useUpdateStore();

  useEffect(() => {
    void hydrateCurrentVersion();
  }, [hydrateCurrentVersion]);

  const busy = checking || downloading || installing;
  const statusCopy =
    status === "checking"
      ? "Checking for updates..."
      : status === "up-to-date"
        ? "You're up to date"
        : status === "available" && availableVersion
          ? `Recall ${availableVersion} is available`
          : status === "downloading"
            ? "Downloading update..."
            : status === "installing"
              ? "Installing update..."
              : status === "restart-needed"
                ? "Update installed. Restarting Recall..."
                : status === "failed"
                  ? "Update failed. Please try again."
                  : "Check for updates when you are ready.";

  return (
    <Section title="Updates">
      <div
        style={{
          padding: "18px 22px",
          background: "var(--surface-2)",
          border: "1px solid var(--border-default)",
          borderRadius: 16,
          marginBottom: 18,
        }}
      >
        <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 18 }}>
          <div>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
              {statusCopy}
            </div>
            <div style={{ marginTop: 6, fontSize: 13, color: "var(--text-muted)", lineHeight: 1.6 }}>
              Current version {currentVersion ?? "loading..."}
              {lastCheckedAt ? ` · Last checked ${formatLongTimestamp(lastCheckedAt)}` : " · Never checked"}
            </div>
            {pubDate && updateAvailable && (
              <div style={{ marginTop: 4, fontSize: 12, color: "var(--t-3)" }}>
                Published {formatLongTimestamp(pubDate)}
              </div>
            )}
          </div>

          <button
            className="btn-primary"
            onClick={() => void checkForUpdates()}
            disabled={busy}
            style={{ flexShrink: 0 }}
          >
            <RefreshCw size={13} className={checking ? "animate-spin" : ""} />
            {checking ? "Checking..." : "Check for updates"}
          </button>
        </div>

        {updateAvailable && (
          <div
            style={{
              marginTop: 18,
              padding: 16,
              borderRadius: 14,
              background: "rgba(79,124,255,0.08)",
              border: "1px solid rgba(79,124,255,0.18)",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>
              Recall {availableVersion} is ready to install
            </div>
            {releaseNotes && (
              <div style={{ marginTop: 10, fontSize: 13, lineHeight: 1.6, color: "var(--text-muted)", whiteSpace: "pre-wrap" }}>
                {releaseNotes}
              </div>
            )}
            {(downloading || installing) && (
              <div style={{ marginTop: 14 }}>
                <div style={{ height: 6, borderRadius: 999, background: "rgba(255,255,255,0.08)", overflow: "hidden" }}>
                  <div
                    style={{
                      height: "100%",
                      width: `${downloadProgress}%`,
                      borderRadius: 999,
                      background: "var(--blue)",
                      transition: "width 160ms ease",
                    }}
                  />
                </div>
                <div style={{ marginTop: 7, fontSize: 12, color: "var(--t-3)" }}>
                  {installing ? "Installing..." : `${downloadProgress}% downloaded`}
                </div>
              </div>
            )}
            <button
              className="btn-primary"
              onClick={() => void downloadAndInstallUpdate()}
              disabled={busy}
              style={{ marginTop: 14 }}
            >
              <Download size={13} />
              {downloading ? "Downloading..." : installing ? "Installing..." : "Download and install"}
            </button>
          </div>
        )}

        {error && (
          <div style={{ marginTop: 14, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14 }}>
            <div style={{ fontSize: 13, color: "var(--danger)" }}>{error}</div>
            <button className="btn-ghost" onClick={resetError}>Dismiss</button>
          </div>
        )}
      </div>

      <Toggle
        label="Check on Startup"
        description="Look for updates quietly when Recall opens. Updates are never installed automatically."
        value={settings.updateAutoCheckEnabled}
        onChange={v => void updateSettings({ ...settings, updateAutoCheckEnabled: v })}
      />

      <div style={{ marginTop: 16, fontSize: 12, lineHeight: 1.6, color: "var(--t-3)" }}>
        Recall uses Tauri's signed updater with a static manifest. Update downloads are verified before installation.
      </div>
    </Section>
  );
}

/* ─── License ───────────────────────────────────────────────────── */
function LicenseTab() {
  const { license } = useSettingsStore();
  const activateKey = useLicenseStore((state) => state.activateKey);
  const clearLicense = useLicenseStore((state) => state.clearLicense);
  const licenseStatus = useLicenseStore((state) => state.status);
  const [key,        setKey]        = useState("");
  const [msg,        setMsg]        = useState("");

  async function activate() {
    if (!key.trim()) return;
    const res = await activateKey(key.trim());
    setMsg(res.ok ? "Trial activated successfully." : res.error || "Activation failed.");
    if (res.ok) setKey("");
  }

  const activating = licenseStatus === "validating";

  return (
    <Section title="License">
      {/* Status card */}
      <div style={{
        padding: "18px 22px",
        background: "var(--surface-2)",
        border: `1px solid ${license?.isActivated ? "var(--blue-border)" : "var(--border-default)"}`,
        borderRadius: 16,
        marginBottom: 22,
        display: "flex",
        alignItems: "center",
        gap: 16,
      }}>
        {license?.isActivated
          ? <CheckCircle size={22} color="var(--blue)" />
          : <XCircle     size={22} color="var(--t-4)" />
        }
        <div>
          <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
            {license?.isActivated ? (license.isTrial ? "Trial Active" : "License Active") : "Free Version"}
          </div>
          <div style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 2 }}>
            {license?.isActivated
              ? `${license.activatedAt ? `Activated ${formatLongTimestamp(license.activatedAt)}` : "Activated"}${license.expiresAt ? ` · Expires ${formatLongTimestamp(license.expiresAt)}` : ""}`
              : "Enter a license key to unlock all features"}
          </div>
        </div>
        {license?.isActivated && (
          <button className="btn-danger" style={{ marginLeft: "auto" }} onClick={() => void clearLicense()}>
            Deactivate
          </button>
        )}
      </div>

      {!license?.isActivated && (
        <>
          <div style={{ fontSize: 13, color: "var(--text-muted)", marginBottom: 8 }}>License key</div>
          <div style={{ display: "flex", gap: 10 }}>
            <input
              value={key}
              onChange={e => setKey(e.target.value)}
              onKeyDown={e => e.key === "Enter" && void activate()}
              placeholder="RECALL-XXXX-XXXX-XXXX"
              className="r-input"
              style={{ fontFamily: "monospace", letterSpacing: "0.05em", flex: 1 }}
            />
            <button className="btn-primary" onClick={activate} disabled={!key.trim() || activating}>
              {activating ? "Activating…" : "Activate"}
            </button>
          </div>
          {msg && <div style={{ marginTop: 10, fontSize: 13, color: msg.includes("activated") ? "var(--success)" : "var(--danger)" }}>{msg}</div>}
        </>
      )}
    </Section>
  );
}

/* ─── Shared primitives ─────────────────────────────────────────── */
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h2 style={{ fontSize: 19, fontWeight: 700, color: "var(--text-primary)", letterSpacing: "-0.01em" }}>{title}</h2>
      <div className="accent-line" style={{ marginBottom: 24 }} />
      {children}
    </div>
  );
}

function Toggle({ label, description, value, onChange }: { label: string; description: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "16px 0", borderBottom: "1px solid rgba(255,255,255,0.05)" }}>
      <div>
        <div style={{ fontSize: 14, fontWeight: 500, color: "var(--text-primary)", marginBottom: 2 }}>{label}</div>
        <div style={{ fontSize: 13, color: "var(--text-muted)" }}>{description}</div>
      </div>
      <button className={`toggle ${value ? "on" : ""}`} onClick={() => onChange(!value)}>
        <div className="toggle-thumb" />
      </button>
    </div>
  );
}

function SettingRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "14px 0", borderBottom: "1px solid rgba(255,255,255,0.05)" }}>
      <div style={{ fontSize: 14, color: "var(--text-primary)" }}>{label}</div>
      {children}
    </div>
  );
}

function PillBtn({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        padding: "6px 13px",
        borderRadius: 8,
        fontSize: 13,
        fontWeight: active ? 600 : 400,
        color: active ? "var(--blue)" : "var(--text-muted)",
        background: active ? "var(--blue-dim)" : "var(--surface-2)",
        border: `1px solid ${active ? "var(--blue-border)" : "var(--border-default)"}`,
        cursor: "pointer",
        fontFamily: "inherit",
        transition: "all 120ms",
      }}
    >
      {label}
    </button>
  );
}

function keyboardEventToAccelerator(event: React.KeyboardEvent<HTMLButtonElement>) {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");

  const key = normalizeShortcutKey(event.key);
  if (!key) return null;

  if (parts.length === 0) {
    return null;
  }

  return [...parts, key].join("+");
}

function normalizeShortcutKey(key: string) {
  const lowered = key.toLowerCase();
  if (["control", "shift", "alt", "meta"].includes(lowered)) {
    return null;
  }
  if (lowered === " ") return "Space";
  if (lowered === "escape") return "Esc";
  if (lowered === "arrowup") return "Up";
  if (lowered === "arrowdown") return "Down";
  if (lowered === "arrowleft") return "Left";
  if (lowered === "arrowright") return "Right";
  if (lowered === "pageup") return "PageUp";
  if (lowered === "pagedown") return "PageDown";
  if (lowered === "return") return "Enter";
  if (key.length === 1) return key.toUpperCase();
  return key.charAt(0).toUpperCase() + key.slice(1);
}
