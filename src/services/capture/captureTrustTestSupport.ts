import type {
  BookmarkSyncSummary,
  BootstrapPayload,
  Memory,
  MemoryInput,
} from "@/domain/types";
import {
  captureTrustBaseBootstrapPayload,
  captureTrustCases,
  captureTrustProjects,
  type CaptureTrustCase,
} from "@/services/capture/captureTrustFixtures";
import type {
  CaptureTrustCaseResult,
  CaptureTrustQueryResult,
} from "@/services/capture/captureTrustRunner";
import { syncBookmarksNow } from "@/services/bookmarks";
import {
  getCompletedCaptureTraces,
  getLatestCompletedCaptureTrace,
  markUiConfirmationShown,
  resetCaptureTelemetry,
} from "@/services/capture/captureTelemetry";
import { resetCaptureSyncState } from "@/services/capture/captureSync";
import { searchMemories } from "@/services/search/searchMemories";
import { useAppStore } from "@/stores/appStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSearchStore } from "@/stores/searchStore";
import { useSettingsStore } from "@/stores/settingsStore";

export interface CaptureTrustBackendAdapter {
  reset: () => void;
  enqueueCreateResponses: (memories: Memory[]) => void;
  setBootstrapPayload: (payload: BootstrapPayload) => void;
  setBookmarkSyncSummary: (summary: BookmarkSyncSummary) => void;
  getCreateCallCount: () => number;
}

const defaultBootstrapPayload = captureTrustBaseBootstrapPayload([]);

export const resetRecallStoresForCaptureTrust = () => {
  resetCaptureTelemetry();
  resetCaptureSyncState();

  useMemoryStore.setState({
    memories: [],
    filters: { projectId: "all", sortOrder: "newest", text: "" },
    selectedMemoryId: null,
    operationMessage: null,
  });
  useProjectStore.setState({
    projects: captureTrustProjects,
    activeProjectId: "all",
  });
  useSearchStore.setState({
    query: "",
    results: [],
    selectedIndex: 0,
  });
  useSettingsStore.setState({
    settings: defaultBootstrapPayload.settings,
    shortcuts: defaultBootstrapPayload.shortcuts,
    license: defaultBootstrapPayload.license,
  });
  useAppStore.setState({
    isBootstrapping: false,
    initialized: true,
    runtime: defaultBootstrapPayload.runtime,
    error: null,
  });
};

const rankForQuery = (query: string, expectedId: string) => {
  const results = searchMemories(
    useMemoryStore.getState().memories,
    useProjectStore.getState().projects,
    { text: query, limit: 12 },
  );
  const index = results.findIndex((result) => result.memory.id === expectedId);
  return index >= 0 ? index + 1 : null;
};

const buildQueryResults = (
  queries: Array<{ label: string; query: string; maxRank: number; expectedId: string }>,
): CaptureTrustQueryResult[] =>
  queries.map((query) => {
    const rank = rankForQuery(query.query, query.expectedId);
    return {
      ...query,
      rank,
      passed: rank !== null && rank <= query.maxRank,
    };
  });

const buildFailureResult = (
  testCase: CaptureTrustCase,
  message: string,
): CaptureTrustCaseResult => ({
  id: testCase.id,
  label: testCase.label,
  passed: false,
  trace: getLatestCompletedCaptureTrace(),
  message,
  queryResults: [],
  persistenceCount: useMemoryStore.getState().memories.length,
});

const assertTraceHealthy = (traceId?: string | null) => {
  const trace = traceId ? getCompletedCaptureTraces().find((item) => item.traceId === traceId) : null;
  if (!trace) return null;
  return trace;
};

const createMemoryThroughStore = async (
  input: MemoryInput,
  origin: "quick-capture" | "shortcut" | "manual",
  showUiConfirmation = false,
) => {
  const result = await useMemoryStore.getState().create(input, { origin });
  if (result.ok && result.traceId && showUiConfirmation) {
    markUiConfirmationShown(result.traceId);
  }
  return result;
};

export const executeCaptureTrustCase = async (
  testCase: CaptureTrustCase,
  backend: CaptureTrustBackendAdapter,
): Promise<CaptureTrustCaseResult> => {
  resetRecallStoresForCaptureTrust();
  backend.reset();
  backend.setBootstrapPayload(defaultBootstrapPayload);

  if (testCase.kind === "manual") {
    backend.enqueueCreateResponses([testCase.persistedMemory]);
    const result = await createMemoryThroughStore(
      testCase.input,
      testCase.origin,
      testCase.showUiConfirmation ?? false,
    );
    const trace = assertTraceHealthy(result.traceId);
    const queryResults = buildQueryResults(testCase.queries);
    const passed =
      result.ok &&
      !!trace &&
      useMemoryStore.getState().memories.some((memory) => memory.id === testCase.persistedMemory.id) &&
      queryResults.every((query) => query.passed) &&
      trace.latencyPass.dbWrite !== false &&
      trace.latencyPass.searchVisible !== false &&
      trace.latencyPass.fullPropagation !== false;

    return {
      id: testCase.id,
      label: testCase.label,
      passed,
      trace,
      message: result.ok
        ? "Memory saved, visible in the list, and immediately retrievable."
        : result.error ?? "Manual capture failed.",
      queryResults,
      persistenceCount: useMemoryStore.getState().memories.length,
    };
  }

  if (testCase.kind === "empty") {
    const result = await createMemoryThroughStore(testCase.input, testCase.origin);
    const trace = assertTraceHealthy(result.traceId);
    const passed =
      !result.ok &&
      result.error === testCase.expectedError &&
      backend.getCreateCallCount() === 0 &&
      useMemoryStore.getState().memories.length === 0 &&
      trace?.status === "failure";

    return {
      id: testCase.id,
      label: testCase.label,
      passed,
      trace: trace ?? null,
      message: result.ok ? "Expected rejection, but save succeeded." : result.error ?? testCase.expectedError,
      queryResults: [],
      persistenceCount: useMemoryStore.getState().memories.length,
    };
  }

  if (testCase.kind === "bookmark") {
    backend.setBootstrapPayload(testCase.bootstrapPayload);
    backend.setBookmarkSyncSummary(testCase.summary);
    const result = await syncBookmarksNow();
    const trace = assertTraceHealthy(result.traceId);
    const queryResults = buildQueryResults(testCase.queries);
    const passed =
      result.ok &&
      !!trace &&
      queryResults.every((query) => query.passed) &&
      trace.latencyPass.dbWrite !== false &&
      trace.latencyPass.searchVisible !== false &&
      trace.latencyPass.fullPropagation !== false;

    return {
      id: testCase.id,
      label: testCase.label,
      passed,
      trace,
      message: result.ok
        ? `Imported ${result.data?.totalImported ?? 0} bookmark(s) and refreshed local search state.`
        : result.error ?? "Bookmark sync failed.",
      queryResults,
      persistenceCount: useMemoryStore.getState().memories.length,
    };
  }

  if (testCase.kind === "rapid") {
    backend.enqueueCreateResponses(testCase.captures.map((capture) => capture.persistedMemory));
    const results = await Promise.all(
      testCase.captures.map((capture) =>
        createMemoryThroughStore(capture.input, "quick-capture"),
      ),
    );

    const traces = getCompletedCaptureTraces();
    const queryResults = buildQueryResults(testCase.captures.flatMap((capture) => capture.queries));
    const allOk = results.every((result) => result.ok);
    const passed =
      allOk &&
      traces.length === testCase.captures.length &&
      useMemoryStore.getState().memories.length === testCase.captures.length &&
      queryResults.every((query) => query.passed) &&
      traces.every(
        (trace) =>
          trace.latencyPass.dbWrite !== false &&
          trace.latencyPass.searchVisible !== false &&
          trace.latencyPass.fullPropagation !== false,
      );

    return {
      id: testCase.id,
      label: testCase.label,
      passed,
      trace: traces.length > 0 ? traces[traces.length - 1] : null,
      message: allOk
        ? `Saved ${testCase.captures.length} captures back-to-back without losing search visibility.`
        : "One or more rapid captures failed.",
      queryResults,
      persistenceCount: useMemoryStore.getState().memories.length,
    };
  }

  return buildFailureResult(testCase, "Unhandled capture trust case.");
};

export const captureTrustScenarioCount = captureTrustCases.length;
