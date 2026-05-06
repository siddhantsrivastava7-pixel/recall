import { invoke } from "@tauri-apps/api/core";
import type {
  AppContextSnapshot,
  AppSettings,
  BookmarkBrowser,
  BookmarkSourceStatus,
  BookmarkSyncSummary,
  BootstrapPayload,
  LicenseState,
  Memory,
  MemoryInput,
  PairingInfo,
  Project,
  RuntimeInfo,
  ShortcutBinding,
} from "@/domain/types";

export interface LicenseValidationResult {
  valid: boolean;
  expired: boolean;
}

export const tauriClient = {
  bootstrap: () => invoke<BootstrapPayload>("bootstrap_app"),
  listMemories: () => invoke<Memory[]>("list_memories"),
  getMemory: (id: string) => invoke<Memory | null>("get_memory", { id }),
  createMemory: (input: MemoryInput) => invoke<Memory>("create_memory", { input }),
  updateMemory: (id: string, input: MemoryInput) =>
    invoke<Memory>("update_memory", { id, input }),
  markMemoryOpened: (id: string) => invoke<Memory | null>("mark_memory_opened", { id }),
  setMemoryResurface: (id: string, resurfaceAt: string | null) =>
    invoke<Memory | null>("set_memory_resurface", { id, resurfaceAt }),
  dismissMemoryResurface: (id: string) =>
    invoke<Memory | null>("dismiss_memory_resurface", { id }),
  deleteMemory: (id: string) => invoke<void>("delete_memory", { id }),
  duplicateMemory: (id: string) => invoke<Memory>("duplicate_memory", { id }),
  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string, description: string | null) =>
    invoke<Project>("create_project", { name, description }),
  updateProject: (id: string, name: string, description: string | null) =>
    invoke<Project>("update_project", { id, name, description }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  getSettings: () => invoke<AppSettings>("get_settings"),
  updateSettings: (settings: AppSettings) => invoke<AppSettings>("update_settings", { settings }),
  listBookmarkSources: () => invoke<BookmarkSourceStatus[]>("list_bookmark_sources"),
  importBookmarks: (browsers: BookmarkBrowser[]) =>
    invoke<BookmarkSyncSummary>("import_bookmarks", { browsers }),
  syncBookmarksNow: () => invoke<BookmarkSyncSummary>("sync_bookmarks_now"),

  // v0.5.37 — X (Twitter) bookmark sync via OAuth 2.0 PKCE.
  xConnectionStatus: () => invoke<XOAuthRow | null>("x_connection_status"),
  xOauthStart: () => invoke<XOAuthRow>("x_oauth_start"),
  xSyncBookmarksNow: () =>
    invoke<XBookmarkSyncResult>("x_sync_bookmarks_now"),
  xOauthDisconnect: () => invoke<void>("x_oauth_disconnect"),
  readClipboardText: () => invoke<string | null>("read_clipboard_text"),
  writeClipboardText: (text: string) => invoke<void>("write_clipboard_text", { text }),
  detectAppContext: () => invoke<AppContextSnapshot>("detect_app_context"),
  getRuntimeInfo: () => invoke<RuntimeInfo>("get_runtime_info"),
  exportData: () => invoke<string>("export_data"),
  importData: () => invoke<string>("import_data"),
  clearAllData: () => invoke<void>("clear_all_data"),
  listShortcuts: () => invoke<ShortcutBinding[]>("list_shortcuts"),
  updateShortcuts: (shortcuts: ShortcutBinding[]) =>
    invoke<ShortcutBinding[]>("update_shortcuts", { shortcuts }),
  activateLicense: (licenseKey: string) =>
    invoke<LicenseState>("activate_license", { licenseKey }),
  validateLicenseKey: (licenseKey: string) =>
    invoke<LicenseValidationResult>("validate_license_key", { licenseKey }),
  deactivateLicense: () => invoke<LicenseState>("deactivate_license"),
  getLicenseState: () => invoke<LicenseState>("get_license_state"),
  getPairingInfo: () => invoke<PairingInfo>("get_pairing_info"),
  resetPairing: () => invoke<PairingInfo>("reset_pairing"),
  openMainWindow: () => invoke<void>("open_main_window"),
  openSearchOverlay: () => invoke<void>("open_search_overlay"),
  openQuickSaveWindow: () => invoke<void>("open_quick_save_window"),
  openMemoryInMain: (memoryId: string) => invoke<void>("open_memory_in_main", { memoryId }),
  closeCurrentWindow: () => invoke<void>("close_current_window"),
  setWidgetExpanded: (expanded: boolean) => invoke<void>("set_widget_expanded", { expanded }),
  saveWidgetPosition: () => invoke<void>("save_widget_position"),
  seedSampleData: () => invoke<void>("seed_sample_data"),
};

// v0.5.37 — X OAuth shapes mirrored from
// `src-tauri/src/services/x_bookmark_sync.rs::XOAuthRow` and
// `BookmarkSyncResult`. Camel-case field names match the
// `#[serde(rename_all = "camelCase")]` annotation on the Rust
// structs.
export interface XOAuthRow {
  id: string;
  xUserId: string;
  xUsername: string | null;
  accessToken: string;
  refreshToken: string | null;
  expiresAt: string | null;
  scope: string | null;
  connectedAt: string;
  lastSyncedAt: string | null;
  lastSyncCount: number;
}

export interface XBookmarkSyncResult {
  fetched: number;
  created: number;
  alreadySaved: number;
}
