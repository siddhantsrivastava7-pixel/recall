export type WindowLabel = "main" | "widget" | "search-overlay" | "quick-save";
export type RuntimePlatform = "windows" | "macos" | "linux" | "unknown";
export type SearchStrategy = "keyword" | "semantic";
export type MemorySourceType = "manual" | "bookmark";
export type BookmarkBrowser = "chrome" | "edge" | "brave" | "safari";
export type LinkEnrichmentStatus = "pending" | "done" | "failed";
export type MemoryType =
  | "article"
  | "docs"
  | "tool"
  | "bookmark"
  | "note"
  | "code_snippet"
  | "video"
  | "post";

export interface Project {
  id: string;
  name: string;
  description: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Memory {
  id: string;
  sourceType: MemorySourceType;
  title: string | null;
  content: string;
  note: string | null;
  projectId: string | null;
  projectName: string | null;
  url: string | null;
  domain?: string | null;
  resolvedDomain?: string | null;
  canonicalUrl?: string | null;
  resolvedTitle?: string | null;
  resolvedDescription?: string | null;
  resolvedImage?: string | null;
  resolvedSiteName?: string | null;
  previewText?: string | null;
  summaryText?: string | null;
  extractedText?: string | null;
  memoryType?: MemoryType | null;
  topicLabels?: string[] | null;
  primaryTopic?: string | null;
  qualityScore?: number | null;
  bookmarkQualityScore?: number | null;
  isDuplicateOf?: string | null;
  bookmarkFolderPath?: string | null;
  enrichmentStatus?: LinkEnrichmentStatus | null;
  enrichmentError?: string | null;
  enrichedAt?: string | null;
  lastEnrichedAt?: string | null;
  externalId: string | null;
  folderPath: string | null;
  sourceApp: string | null;
  sourceWindow: string | null;
  resurfaceAt?: string | null;
  resurfaceDismissedAt?: string | null;
  lastOpenedAt?: string | null;
  openCount?: number;
  // v0.2.0 — OCR fields. Populated by the AI scheduler on screenshot /
  // imported_image memories. Other memories carry these as `null`.
  ocrText?: string | null;
  ocrStatus?: "pending" | "running" | "done" | "failed" | null;
  ocrProcessedAt?: string | null;
  ocrEngine?: string | null;
  ocrError?: string | null;
  // v0.3.0 — embedding state. `embeddingGeneratedAt` bumps when any of
  // this memory's chunks gets a fresh embedding; `RelatedMemories`
  // uses it as the dependency for re-querying related results.
  embeddingModelVersion?: string | null;
  embeddingGeneratedAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ShortcutBinding {
  action: ShortcutAction;
  accelerator: string;
  editable: boolean;
  description: string;
}

export interface PairingInfo {
  deviceId: string;
  pairingSecret: string;
  desktopName: string;
  endpoint: string | null;
  port: number | null;
  createdAt: string;
  receiverRunning: boolean;
  pairingStatus: "ready" | "not_running" | string;
  qrPayload: string;
}

export interface PairingQrPayload {
  protocol: "recall-local-pairing";
  version: 1;
  deviceId: string;
  desktopName: string;
  endpoint: string | null;
  secret: string;
}

export type ShortcutAction =
  | "open-search"
  | "open-quick-save"
  | "open-main-app";

export interface AppSettings {
  floatingWidgetEnabled: boolean;
  launchOnStartupEnabled: boolean;
  updateAutoCheckEnabled: boolean;
  bookmarkAutoSyncEnabled: boolean;
  bookmarkSyncIntervalMinutes: number;
  bookmarkSyncBrowsers: BookmarkBrowser[];
  bookmarkLastSyncedAt: string | null;
  // v0.2.0 — AI subsystem master switches. All default to safe values
  // (off / pause-on-battery / heavy-only-on-AC) so existing users see
  // zero behavior change after the update until they opt in.
  aiEnabled: boolean;
  aiPauseOnBattery: boolean;
  aiHeavyOnlyOnAc: boolean;
}

// ── AI subsystem (v0.2.0+) ───────────────────────────────────────────
export type HardwareTier = "a" | "b" | "c";
export type CpuArch = "applesilicon" | "x86_64" | "other";
export type OsKind = "macos" | "windows" | "other";

export interface HardwareInfo {
  tier: HardwareTier;
  totalRamBytes: number;
  cpuCores: number;
  arch: CpuArch;
  os: OsKind;
}

export interface SchedulerStatus {
  enabled: boolean;
  ocrQueued: number;
  ocrRunning: number;
  ocrFailed: number;
  // v0.3.0
  embedQueued: number;
  embedRunning: number;
  embedFailed: number;
}

export interface EmbeddingCoverage {
  totalMemories: number;
  memoriesWithChunks: number;
  totalChunks: number;
  embeddedChunks: number;
  /// v0.3.3 — chunks embedded under the *currently-active* model.
  /// Lags `embeddedChunks` after a model upgrade until the user runs
  /// "Embed all memories"; the gap drives the upgrade banner.
  embeddedChunksActiveModel: number;
}

export interface AiStatusPayload {
  enabled: boolean;
  hardware: HardwareInfo;
  ocrEngine: string;
  ocrAvailable: boolean;
  // v0.3.0
  embeddingModel: string;
  embeddingReady: boolean;
  embeddingCoverage: EmbeddingCoverage;
  queue: SchedulerStatus;
}

export interface LicenseState {
  id: string;
  licenseKey: string | null;
  isActivated: boolean;
  isTrial: boolean;
  activatedAt: string | null;
  expiresAt: string | null;
  lastCheckedAt: string | null;
}

export interface RuntimeInfo {
  platform: RuntimePlatform;
  currentWindowLabel: WindowLabel;
  databasePath: string;
}

export interface AppContextSnapshot {
  sourceApp: string | null;
  sourceWindow: string | null;
}

export interface BootstrapPayload {
  runtime: RuntimeInfo;
  settings: AppSettings;
  license: LicenseState;
  memories: Memory[];
  projects: Project[];
  shortcuts: ShortcutBinding[];
}

export interface MemoryInput {
  sourceType?: MemorySourceType | null;
  title: string | null;
  content: string;
  note: string | null;
  projectId: string | null;
  url?: string | null;
  externalId?: string | null;
  folderPath?: string | null;
  sourceApp: string | null;
  sourceWindow: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
}

export interface MemoryFilters {
  projectId: string | "all";
  sortOrder: "newest" | "oldest";
  text: string;
}

export interface SearchResult {
  memory: Memory;
  score: number;
  highlights: string[];
  strategy?: SearchStrategy;
  providerId?: string;
}

export interface SearchSuggestion {
  memory: Memory;
  score: number;
  reason: string;
  matchedTopics: string[];
}

export interface SearchQuery {
  text: string;
  projectId?: string | null;
  limit?: number;
}

export interface QuickSaveDraft {
  title: string;
  content: string;
  note: string;
  projectId: string;
  sourceApp: string | null;
  sourceWindow: string | null;
  clipboardWasEmpty: boolean;
}

export interface BackupPayload {
  exportedAt: string;
  version: string;
  memories: Memory[];
  projects: Project[];
  settings: AppSettings;
  license: LicenseState;
}

export interface BookmarkSourceStatus {
  browser: BookmarkBrowser;
  path: string | null;
  isAvailable: boolean;
}

export interface BookmarkImportResult {
  browser: BookmarkBrowser;
  path: string | null;
  importedCount: number;
  skippedCount: number;
  message: string;
}

export interface BookmarkSyncSummary {
  results: BookmarkImportResult[];
  totalImported: number;
  totalSkipped: number;
  syncedAt: string | null;
}

export interface ServiceResult<T> {
  ok: boolean;
  data?: T;
  error?: string;
}
