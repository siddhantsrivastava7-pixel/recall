import type {
  AppSettings,
  BookmarkSyncSummary,
  BootstrapPayload,
  LicenseState,
  Memory,
  MemoryInput,
  Project,
  RuntimeInfo,
  ShortcutBinding,
} from "@/domain/types";

export interface CaptureTrustQueryExpectation {
  label: string;
  query: string;
  maxRank: number;
  expectedId: string;
}

export interface ManualCaptureTrustCase {
  id: string;
  label: string;
  kind: "manual";
  origin: "quick-capture" | "shortcut";
  input: MemoryInput;
  persistedMemory: Memory;
  queries: CaptureTrustQueryExpectation[];
  showUiConfirmation?: boolean;
}

export interface EmptyCaptureTrustCase {
  id: string;
  label: string;
  kind: "empty";
  origin: "quick-capture" | "shortcut" | "manual";
  input: MemoryInput;
  expectedError: string;
}

export interface BookmarkCaptureTrustCase {
  id: string;
  label: string;
  kind: "bookmark";
  summary: BookmarkSyncSummary;
  bootstrapPayload: BootstrapPayload;
  queries: CaptureTrustQueryExpectation[];
}

export interface RapidCaptureTrustCase {
  id: string;
  label: string;
  kind: "rapid";
  captures: Array<{
    input: MemoryInput;
    persistedMemory: Memory;
    queries: CaptureTrustQueryExpectation[];
  }>;
}

export type CaptureTrustCase =
  | ManualCaptureTrustCase
  | EmptyCaptureTrustCase
  | BookmarkCaptureTrustCase
  | RapidCaptureTrustCase;

export const captureTrustLatencyThresholds = {
  dbWriteMs: 300,
  searchVisibleMs: 100,
  fullPropagationMs: 400,
  uiConfirmationMs: 800,
} as const;

const runtime: RuntimeInfo = {
  platform: "windows",
  currentWindowLabel: "main",
  databasePath: "C:\\Recall\\recall.db",
};

const settings: AppSettings = {
  floatingWidgetEnabled: true,
  launchOnStartupEnabled: false,
  updateAutoCheckEnabled: true,
  bookmarkAutoSyncEnabled: true,
  bookmarkSyncIntervalMinutes: 15,
  bookmarkSyncBrowsers: ["chrome", "edge", "brave"],
  bookmarkLastSyncedAt: null,
};

const shortcuts: ShortcutBinding[] = [];

const license: LicenseState = {
  id: "license-local",
  licenseKey: "RC-TEST-TEST-F",
  isActivated: true,
  activatedAt: "2026-04-09T11:30:00.000Z",
  lastCheckedAt: "2026-04-09T11:30:00.000Z",
};

const projects: Project[] = [
  {
    id: "system-inbox",
    name: "Inbox",
    description: null,
    createdAt: "2026-04-01T09:00:00.000Z",
    updatedAt: "2026-04-01T09:00:00.000Z",
  },
  {
    id: "project-pricing",
    name: "Pricing",
    description: "Pricing and packaging work",
    createdAt: "2026-04-02T10:00:00.000Z",
    updatedAt: "2026-04-02T10:00:00.000Z",
  },
  {
    id: "project-marketing",
    name: "Marketing",
    description: "Positioning and messaging",
    createdAt: "2026-04-03T10:00:00.000Z",
    updatedAt: "2026-04-03T10:00:00.000Z",
  },
];

const baseBootstrapPayload = (memories: Memory[]): BootstrapPayload => ({
  runtime,
  settings,
  license,
  memories,
  projects,
  shortcuts,
});

const quickCaptureMemory: Memory = {
  id: "memory-quick-1",
  sourceType: "manual",
  title: "Investor pricing framework",
  content:
    "Investor pricing framework for enterprise renewals and packaging guardrails.",
  note: null,
  projectId: "system-inbox",
  projectName: "Inbox",
  url: null,
  externalId: null,
  folderPath: null,
  sourceApp: "Recall",
  sourceWindow: "Quick Capture",
  createdAt: "2026-04-09T12:00:00.000Z",
  updatedAt: "2026-04-09T12:00:00.000Z",
};

const shortcutMemory: Memory = {
  id: "memory-shortcut-1",
  sourceType: "manual",
  title: "https://docs.render.com/deploy/edge-caching",
  content: "https://docs.render.com/deploy/edge-caching",
  note: "Keep the CDN invalidation note for launch week.",
  projectId: "system-inbox",
  projectName: "Inbox",
  url: "https://docs.render.com/deploy/edge-caching",
  externalId: null,
  folderPath: null,
  sourceApp: "Brave",
  sourceWindow: "Render Docs",
  createdAt: "2026-04-09T12:01:00.000Z",
  updatedAt: "2026-04-09T12:01:00.000Z",
};

const metadataMemory: Memory = {
  id: "memory-meta-1",
  sourceType: "manual",
  title: "Landing page launch hooks",
  content: "Landing page launch hooks focused on conversion clarity and premium trust.",
  note: "Use the calmer Apple-style hero framing from the review notes.",
  projectId: "project-marketing",
  projectName: "Marketing",
  url: null,
  externalId: null,
  folderPath: null,
  sourceApp: "Chrome",
  sourceWindow: "Figma",
  createdAt: "2026-04-09T12:02:00.000Z",
  updatedAt: "2026-04-09T12:02:00.000Z",
};

const importedBookmark: Memory = {
  id: "bookmark-memory-1",
  sourceType: "bookmark",
  title: "OpenAI pricing docs",
  content: "https://platform.openai.com/docs/pricing",
  note: null,
  projectId: "system-inbox",
  projectName: "Inbox",
  url: "https://platform.openai.com/docs/pricing",
  externalId: "bookmark-1",
  folderPath: "Bookmarks Bar / Research / API",
  sourceApp: "chrome",
  sourceWindow: null,
  createdAt: "2026-04-01T09:00:00.000Z",
  updatedAt: "2026-04-01T09:00:00.000Z",
};

export const rapidCaptureMemories: Memory[] = [
  {
    id: "memory-rapid-1",
    sourceType: "manual",
    title: "Pricing objection handling",
    content: "Pricing objection handling for procurement conversations.",
    note: null,
    projectId: "project-pricing",
    projectName: "Pricing",
    url: null,
    externalId: null,
    folderPath: null,
    sourceApp: "Recall",
    sourceWindow: "Quick Capture",
    createdAt: "2026-04-09T12:03:00.000Z",
    updatedAt: "2026-04-09T12:03:00.000Z",
  },
  {
    id: "memory-rapid-2",
    sourceType: "manual",
    title: "Expansion pricing notes",
    content: "Expansion pricing notes for yearly contract upsells.",
    note: null,
    projectId: "project-pricing",
    projectName: "Pricing",
    url: null,
    externalId: null,
    folderPath: null,
    sourceApp: "Recall",
    sourceWindow: "Quick Capture",
    createdAt: "2026-04-09T12:03:01.000Z",
    updatedAt: "2026-04-09T12:03:01.000Z",
  },
  {
    id: "memory-rapid-3",
    sourceType: "manual",
    title: "Renewal packaging script",
    content: "Renewal packaging script for enterprise plan migration calls.",
    note: null,
    projectId: "project-pricing",
    projectName: "Pricing",
    url: null,
    externalId: null,
    folderPath: null,
    sourceApp: "Recall",
    sourceWindow: "Quick Capture",
    createdAt: "2026-04-09T12:03:02.000Z",
    updatedAt: "2026-04-09T12:03:02.000Z",
  },
];

export const captureTrustCases: CaptureTrustCase[] = [
  {
    id: "manual-quick-capture",
    label: "Manual quick capture becomes immediately searchable",
    kind: "manual",
    origin: "quick-capture",
    input: {
      sourceType: "manual",
      title: null,
      content:
        "Investor pricing framework for enterprise renewals and packaging guardrails.",
      note: null,
      projectId: null,
      sourceApp: "Recall",
      sourceWindow: "Quick Capture",
    },
    persistedMemory: quickCaptureMemory,
    showUiConfirmation: true,
    queries: [
      {
        label: "exact_phrase",
        query: "Investor pricing framework for enterprise renewals",
        maxRank: 3,
        expectedId: quickCaptureMemory.id,
      },
      {
        label: "title_phrase",
        query: "Investor pricing framework",
        maxRank: 2,
        expectedId: quickCaptureMemory.id,
      },
      {
        label: "key_content_phrase",
        query: "enterprise renewals and packaging guardrails",
        maxRank: 4,
        expectedId: quickCaptureMemory.id,
      },
    ],
  },
  {
    id: "shortcut-clipboard-capture",
    label: "Shortcut clipboard capture becomes immediately searchable",
    kind: "manual",
    origin: "shortcut",
    input: {
      sourceType: "manual",
      title: null,
      content: "https://docs.render.com/deploy/edge-caching",
      note: "Keep the CDN invalidation note for launch week.",
      projectId: null,
      url: "https://docs.render.com/deploy/edge-caching",
      sourceApp: "Brave",
      sourceWindow: "Render Docs",
    },
    persistedMemory: shortcutMemory,
    queries: [
      {
        label: "exact_phrase",
        query: "https://docs.render.com/deploy/edge-caching",
        maxRank: 2,
        expectedId: shortcutMemory.id,
      },
      {
        label: "title_phrase",
        query: "edge caching",
        maxRank: 3,
        expectedId: shortcutMemory.id,
      },
      {
        label: "key_content_phrase",
        query: "CDN invalidation note",
        maxRank: 4,
        expectedId: shortcutMemory.id,
      },
    ],
  },
  {
    id: "metadata-searchability",
    label: "Saved memory with metadata is searchable by title, project, and note",
    kind: "manual",
    origin: "quick-capture",
    input: {
      sourceType: "manual",
      title: "Landing page launch hooks",
      content: "Landing page launch hooks focused on conversion clarity and premium trust.",
      note: "Use the calmer Apple-style hero framing from the review notes.",
      projectId: "project-marketing",
      sourceApp: "Chrome",
      sourceWindow: "Figma",
    },
    persistedMemory: metadataMemory,
    queries: [
      {
        label: "title_phrase",
        query: "Landing page launch hooks",
        maxRank: 2,
        expectedId: metadataMemory.id,
      },
      {
        label: "project_phrase",
        query: "Marketing",
        maxRank: 4,
        expectedId: metadataMemory.id,
      },
      {
        label: "note_phrase",
        query: "Apple-style hero framing",
        maxRank: 4,
        expectedId: metadataMemory.id,
      },
    ],
  },
  {
    id: "empty-save-rejected",
    label: "Empty save is rejected with no partial persistence",
    kind: "empty",
    origin: "quick-capture",
    input: {
      sourceType: "manual",
      title: null,
      content: "   \n  ",
      note: null,
      projectId: null,
      sourceApp: "Recall",
      sourceWindow: "Quick Capture",
    },
    expectedError: "Content is required.",
  },
  {
    id: "bookmark-import-searchable",
    label: "Imported bookmark becomes searchable immediately after sync",
    kind: "bookmark",
    summary: {
      results: [
        {
          browser: "chrome",
          path: "C:\\Users\\siddh\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Bookmarks",
          importedCount: 1,
          skippedCount: 0,
          message: "Chrome import complete: 1 new, 0 already saved.",
        },
      ],
      totalImported: 1,
      totalSkipped: 0,
      syncedAt: "2026-04-09T12:04:00.000Z",
    },
    bootstrapPayload: baseBootstrapPayload([importedBookmark]),
    queries: [
      {
        label: "exact_phrase",
        query: "https://platform.openai.com/docs/pricing",
        maxRank: 2,
        expectedId: importedBookmark.id,
      },
      {
        label: "title_phrase",
        query: "OpenAI pricing docs",
        maxRank: 2,
        expectedId: importedBookmark.id,
      },
      {
        label: "folder_phrase",
        query: "Research API",
        maxRank: 4,
        expectedId: importedBookmark.id,
      },
    ],
  },
  {
    id: "rapid-repeated-captures",
    label: "Repeated rapid captures remain durable, visible, and searchable",
    kind: "rapid",
    captures: rapidCaptureMemories.map((memory) => ({
      input: {
        sourceType: "manual",
        title: memory.title,
        content: memory.content,
        note: null,
        projectId: memory.projectId,
        sourceApp: memory.sourceApp,
        sourceWindow: memory.sourceWindow,
      },
      persistedMemory: memory,
      queries: [
        {
          label: "title_phrase",
          query: memory.title ?? "",
          maxRank: 2,
          expectedId: memory.id,
        },
      ],
    })),
  },
];

export const captureTrustProjects = projects;
export const captureTrustBaseBootstrapPayload = baseBootstrapPayload;
