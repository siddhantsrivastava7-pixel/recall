import type { Memory, MemorySourceType, Project } from "@/domain/types";
import { searchMemories } from "@/services/search/searchMemories";

export type CaptureTraceOrigin =
  | "quick-capture"
  | "shortcut"
  | "drop-capture"
  | "instant-clipboard"
  | "manual"
  | "bookmark-import";

export type CaptureTraceStage =
  | "capture_start"
  | "db_write_complete"
  | "store_update_complete"
  | "search_visible_complete"
  | "ui_confirmation_shown";

export interface CaptureLatencyThresholds {
  dbWriteMs: number;
  searchVisibleMs: number;
  fullPropagationMs: number;
  uiConfirmationMs: number;
}

export interface CaptureRankThresholds {
  exactPhraseMaxRank: number;
  titlePhraseMaxRank: number;
  keyContentPhraseMaxRank: number;
}

export interface SearchVisibilityCheck {
  label: "exact_phrase" | "title_phrase" | "key_content_phrase";
  query: string;
  rank: number | null;
  maxRank: number;
  passed: boolean;
}

export interface SearchVisibilitySummary {
  checks: SearchVisibilityCheck[];
  passed: boolean;
}

export interface CaptureTraceRecord {
  traceId: string;
  captureId: string | null;
  origin: CaptureTraceOrigin;
  sourceType: MemorySourceType;
  status: "pending" | "success" | "failure";
  stages: Partial<Record<CaptureTraceStage, number>>;
  durations: {
    saveDurationMs: number | null;
    searchVisibilityLatencyMs: number | null;
    fullPropagationLatencyMs: number | null;
    uiConfirmationLatencyMs: number | null;
  };
  latencyThresholds: CaptureLatencyThresholds;
  latencyPass: {
    dbWrite: boolean | null;
    searchVisible: boolean | null;
    fullPropagation: boolean | null;
    uiConfirmation: boolean | null;
  };
  searchChecks: SearchVisibilityCheck[];
  error: string | null;
}

export interface CaptureTraceSummary {
  total: number;
  successful: number;
  failed: number;
  reliabilityScore: number;
  averageDbWriteMs: number;
  maxDbWriteMs: number;
  averageSearchVisibleMs: number;
  maxSearchVisibleMs: number;
  averageFullPropagationMs: number;
  maxFullPropagationMs: number;
}

interface SearchProbe {
  label: SearchVisibilityCheck["label"];
  query: string;
  maxRank: number;
}

const DEFAULT_CAPTURE_LATENCY_THRESHOLDS: CaptureLatencyThresholds = {
  dbWriteMs: 800,
  searchVisibleMs: 150,
  fullPropagationMs: 1000,
  uiConfirmationMs: 1400,
};

const DEFAULT_CAPTURE_RANK_THRESHOLDS: CaptureRankThresholds = {
  exactPhraseMaxRank: 3,
  titlePhraseMaxRank: 2,
  keyContentPhraseMaxRank: 5,
};

const processEnv =
  typeof globalThis === "object" && "process" in globalThis
    ? (
        globalThis as {
          process?: { env?: Record<string, string | undefined> };
        }
      ).process?.env
    : undefined;

const DEBUG_CAPTURE_TRACING =
  processEnv?.VITEST === "true" ||
  processEnv?.NODE_ENV === "development" ||
  (typeof window !== "undefined" && window.location.hostname === "localhost");

const traces = new Map<string, CaptureTraceRecord>();
let traceCounter = 0;

const now = () => performance.now();

const average = (values: number[]) =>
  values.length === 0
    ? 0
    : values.reduce((total, value) => total + value, 0) / values.length;

const max = (values: number[]) => (values.length === 0 ? 0 : Math.max(...values));

const normalizeSnippet = (value: string) => value.replace(/\s+/g, " ").trim();

const unique = <T,>(values: T[]) => Array.from(new Set(values));

const firstSentence = (value: string) => {
  const normalized = normalizeSnippet(value);
  const match = normalized.match(/^(.{1,140}?[.!?])(?:\s|$)/);
  return match?.[1]?.trim() ?? normalized;
};

const firstMeaningfulWords = (value: string, count = 6) => {
  const words = normalizeSnippet(value)
    .split(" ")
    .map((word) => word.trim())
    .filter(Boolean);

  if (words.length === 0) return "";
  return words.slice(0, count).join(" ");
};

const deriveExactPhrase = (memory: Memory) => {
  if (memory.sourceType === "bookmark" && memory.url) {
    return normalizeSnippet(memory.url);
  }
  return firstSentence(memory.content);
};

const deriveKeyContentPhrase = (memory: Memory) => {
  if (memory.sourceType === "bookmark") {
    return normalizeSnippet(memory.folderPath ?? memory.url ?? memory.content);
  }

  const firstLine = memory.content
    .split("\n")
    .map((line) => normalizeSnippet(line))
    .find(Boolean);

  return firstMeaningfulWords(firstLine ?? memory.content, 7);
};

const buildSearchProbes = (
  memory: Memory,
  thresholds: CaptureRankThresholds,
): SearchProbe[] => {
  const probes: SearchProbe[] = [];
  const exactPhrase = deriveExactPhrase(memory);
  const titlePhrase = normalizeSnippet(memory.title ?? "");
  const keyContentPhrase = deriveKeyContentPhrase(memory);

  if (exactPhrase) {
    probes.push({
      label: "exact_phrase",
      query: exactPhrase,
      maxRank: thresholds.exactPhraseMaxRank,
    });
  }

  if (titlePhrase) {
    probes.push({
      label: "title_phrase",
      query: titlePhrase,
      maxRank: thresholds.titlePhraseMaxRank,
    });
  }

  if (keyContentPhrase) {
    probes.push({
      label: "key_content_phrase",
      query: keyContentPhrase,
      maxRank: thresholds.keyContentPhraseMaxRank,
    });
  }

  return unique(
    probes.filter((probe) => probe.query.length > 0).map((probe) => JSON.stringify(probe)),
  ).map((serialized) => JSON.parse(serialized) as SearchProbe);
};

const findRank = (
  memories: Memory[],
  projects: Project[],
  memoryId: string,
  probe: SearchProbe,
) => {
  const results = searchMemories(memories, projects, {
    text: probe.query,
    limit: Math.max(10, probe.maxRank + 5),
  });
  const index = results.findIndex((result) => result.memory.id === memoryId);
  return index >= 0 ? index + 1 : null;
};

const computeDurations = (trace: CaptureTraceRecord) => {
  const captureStart = trace.stages.capture_start;
  const dbWrite = trace.stages.db_write_complete;
  const storeUpdate = trace.stages.store_update_complete;
  const searchVisible = trace.stages.search_visible_complete;
  const uiConfirmation = trace.stages.ui_confirmation_shown;

  trace.durations.saveDurationMs =
    captureStart !== undefined && dbWrite !== undefined ? dbWrite - captureStart : null;
  trace.durations.searchVisibilityLatencyMs =
    storeUpdate !== undefined && searchVisible !== undefined
      ? searchVisible - storeUpdate
      : null;
  trace.durations.fullPropagationLatencyMs =
    captureStart !== undefined && searchVisible !== undefined
      ? searchVisible - captureStart
      : null;
  trace.durations.uiConfirmationLatencyMs =
    captureStart !== undefined && uiConfirmation !== undefined
      ? uiConfirmation - captureStart
      : null;

  trace.latencyPass.dbWrite =
    trace.durations.saveDurationMs === null
      ? null
      : trace.durations.saveDurationMs <= trace.latencyThresholds.dbWriteMs;
  trace.latencyPass.searchVisible =
    trace.durations.searchVisibilityLatencyMs === null
      ? null
      : trace.durations.searchVisibilityLatencyMs <= trace.latencyThresholds.searchVisibleMs;
  trace.latencyPass.fullPropagation =
    trace.durations.fullPropagationLatencyMs === null
      ? null
      : trace.durations.fullPropagationLatencyMs <= trace.latencyThresholds.fullPropagationMs;
  trace.latencyPass.uiConfirmation =
    trace.durations.uiConfirmationLatencyMs === null
      ? null
      : trace.durations.uiConfirmationLatencyMs <= trace.latencyThresholds.uiConfirmationMs;
};

const getTrace = (traceId: string) => {
  const trace = traces.get(traceId);
  if (!trace) {
    throw new Error(`Unknown capture trace: ${traceId}`);
  }
  return trace;
};

const debugLog = (trace: CaptureTraceRecord, message: string) => {
  if (!DEBUG_CAPTURE_TRACING) return;

  const parts = [
    "[recall][capture-trace]",
    `trace=${trace.traceId}`,
    `capture=${trace.captureId ?? "pending"}`,
    `origin=${trace.origin}`,
    `source=${trace.sourceType}`,
    `status=${trace.status}`,
    message,
  ];

  if (trace.error) {
    parts.push(`error=${trace.error}`);
  }

  const save = trace.durations.saveDurationMs;
  const search = trace.durations.searchVisibilityLatencyMs;
  const full = trace.durations.fullPropagationLatencyMs;
  if (save !== null) parts.push(`save_ms=${save.toFixed(2)}`);
  if (search !== null) parts.push(`search_ms=${search.toFixed(2)}`);
  if (full !== null) parts.push(`full_ms=${full.toFixed(2)}`);

  if (trace.searchChecks.length > 0) {
    const checks = trace.searchChecks
      .map((check) => `${check.label}:${check.rank ?? "miss"}/${check.maxRank}`)
      .join(",");
    parts.push(`checks=${checks}`);
  }

  console.log(parts.join(" "));
};

export const resolveCaptureLatencyThresholds = (
  overrides?: Partial<CaptureLatencyThresholds>,
): CaptureLatencyThresholds => ({
  ...DEFAULT_CAPTURE_LATENCY_THRESHOLDS,
  ...overrides,
});

export const resolveCaptureRankThresholds = (
  overrides?: Partial<CaptureRankThresholds>,
): CaptureRankThresholds => ({
  ...DEFAULT_CAPTURE_RANK_THRESHOLDS,
  ...overrides,
});

export const startCaptureTrace = ({
  origin,
  sourceType,
  latencyThresholds,
}: {
  origin: CaptureTraceOrigin;
  sourceType: MemorySourceType;
  latencyThresholds?: Partial<CaptureLatencyThresholds>;
}) => {
  traceCounter += 1;
  const traceId = `capture-${traceCounter}`;
  const trace: CaptureTraceRecord = {
    traceId,
    captureId: null,
    origin,
    sourceType,
    status: "pending",
    stages: { capture_start: now() },
    durations: {
      saveDurationMs: null,
      searchVisibilityLatencyMs: null,
      fullPropagationLatencyMs: null,
      uiConfirmationLatencyMs: null,
    },
    latencyThresholds: resolveCaptureLatencyThresholds(latencyThresholds),
    latencyPass: {
      dbWrite: null,
      searchVisible: null,
      fullPropagation: null,
      uiConfirmation: null,
    },
    searchChecks: [],
    error: null,
  };

  traces.set(traceId, trace);
  debugLog(trace, "start");
  return traceId;
};

export const markDbWriteComplete = (
  traceId: string,
  captureId: string,
) => {
  const trace = getTrace(traceId);
  trace.captureId = captureId;
  trace.stages.db_write_complete ??= now();
  computeDurations(trace);
};

export const markStoreUpdateComplete = (traceId: string) => {
  const trace = getTrace(traceId);
  trace.stages.store_update_complete ??= now();
  computeDurations(trace);
};

export const markSearchVisibleComplete = (
  traceId: string,
  summary: SearchVisibilitySummary,
) => {
  const trace = getTrace(traceId);
  trace.stages.search_visible_complete ??= now();
  trace.searchChecks = summary.checks;
  computeDurations(trace);
};

export const markUiConfirmationShown = (traceId: string) => {
  const trace = getTrace(traceId);
  trace.stages.ui_confirmation_shown ??= now();
  computeDurations(trace);
  debugLog(trace, "ui_confirmation_shown");
};

export const markCaptureFailure = (traceId: string, error: string) => {
  const trace = getTrace(traceId);
  trace.status = "failure";
  trace.error = error;
  computeDurations(trace);
  debugLog(trace, "failure");
};

export const markCaptureSuccess = (traceId: string) => {
  const trace = getTrace(traceId);
  trace.status = "success";
  computeDurations(trace);
  debugLog(trace, "success");
};

export const evaluateSearchVisibilityForMemory = (
  memory: Memory,
  options: {
    memories: Memory[];
    projects: Project[];
    rankThresholds?: Partial<CaptureRankThresholds>;
  },
): SearchVisibilitySummary => {
  const { memories, projects } = options;
  const thresholds = resolveCaptureRankThresholds(options.rankThresholds);
  const probes = buildSearchProbes(memory, thresholds);

  const checks = probes.map((probe) => {
    const rank = findRank(memories, projects, memory.id, probe);
    return {
      label: probe.label,
      query: probe.query,
      rank,
      maxRank: probe.maxRank,
      passed: rank !== null && rank <= probe.maxRank,
    } satisfies SearchVisibilityCheck;
  });

  return {
    checks,
    passed: checks.every((check) => check.passed),
  };
};

export const evaluateSearchVisibilityForMemories = (
  memoriesToCheck: Memory[],
  options: {
    memories: Memory[];
    projects: Project[];
    rankThresholds?: Partial<CaptureRankThresholds>;
  },
): SearchVisibilitySummary => {
  const checks = memoriesToCheck.flatMap((memory) =>
    evaluateSearchVisibilityForMemory(memory, options).checks,
  );

  return {
    checks,
    passed: checks.every((check) => check.passed),
  };
};

export const getCaptureTrace = (traceId: string) => structuredClone(getTrace(traceId));

export const getCompletedCaptureTraces = () =>
  Array.from(traces.values())
    .filter((trace) => trace.status !== "pending")
    .map((trace) => structuredClone(trace));

export const getLatestCompletedCaptureTrace = () => {
  const completed = getCompletedCaptureTraces();
  return completed.length > 0 ? completed[completed.length - 1] : null;
};

export const summarizeCaptureTraces = (
  traceList = getCompletedCaptureTraces(),
): CaptureTraceSummary => {
  const dbWrites = traceList
    .map((trace) => trace.durations.saveDurationMs)
    .filter((value): value is number => value !== null);
  const searchVisibility = traceList
    .map((trace) => trace.durations.searchVisibilityLatencyMs)
    .filter((value): value is number => value !== null);
  const fullPropagation = traceList
    .map((trace) => trace.durations.fullPropagationLatencyMs)
    .filter((value): value is number => value !== null);
  const successful = traceList.filter((trace) => trace.status === "success").length;
  const failed = traceList.length - successful;

  return {
    total: traceList.length,
    successful,
    failed,
    reliabilityScore:
      traceList.length === 0 ? 100 : (successful / traceList.length) * 100,
    averageDbWriteMs: average(dbWrites),
    maxDbWriteMs: max(dbWrites),
    averageSearchVisibleMs: average(searchVisibility),
    maxSearchVisibleMs: max(searchVisibility),
    averageFullPropagationMs: average(fullPropagation),
    maxFullPropagationMs: max(fullPropagation),
  };
};

export const resetCaptureTelemetry = () => {
  traces.clear();
  traceCounter = 0;
};
