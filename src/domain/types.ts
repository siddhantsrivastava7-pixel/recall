export type WindowLabel = "main" | "widget" | "search-overlay" | "quick-save";
export type RuntimePlatform = "windows" | "macos" | "linux" | "unknown";
export type SearchStrategy = "keyword" | "semantic";
export type MemorySourceType = "manual" | "bookmark";
export type BookmarkBrowser = "chrome" | "edge" | "brave";
export type LinkEnrichmentStatus = "pending" | "done" | "failed";

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
  topicLabels?: string[] | null;
  bookmarkQualityScore?: number | null;
  isDuplicateOf?: string | null;
  bookmarkFolderPath?: string | null;
  enrichmentStatus?: LinkEnrichmentStatus | null;
  enrichedAt?: string | null;
  lastEnrichedAt?: string | null;
  externalId: string | null;
  folderPath: string | null;
  sourceApp: string | null;
  sourceWindow: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ShortcutBinding {
  action: ShortcutAction;
  accelerator: string;
  editable: boolean;
  description: string;
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
