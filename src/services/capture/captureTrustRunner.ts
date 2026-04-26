import type {
  CaptureTrustCase,
  CaptureTrustQueryExpectation,
} from "@/services/capture/captureTrustFixtures";
import type {
  CaptureTraceRecord,
  CaptureTraceSummary,
} from "@/services/capture/captureTelemetry";
import { summarizeCaptureTraces } from "@/services/capture/captureTelemetry";

export interface CaptureTrustQueryResult extends CaptureTrustQueryExpectation {
  rank: number | null;
  passed: boolean;
}

export interface CaptureTrustCaseResult {
  id: string;
  label: string;
  passed: boolean;
  trace: CaptureTraceRecord | null;
  message: string;
  queryResults: CaptureTrustQueryResult[];
  persistenceCount: number;
}

export interface CaptureTrustSuiteSummary {
  total: number;
  passed: number;
  failed: number;
  percentage: number;
  trustScore: number;
  latency: CaptureTraceSummary;
  results: CaptureTrustCaseResult[];
}

export const runCaptureTrustSuite = async (
  cases: CaptureTrustCase[],
  executeCase: (testCase: CaptureTrustCase) => Promise<CaptureTrustCaseResult>,
): Promise<CaptureTrustSuiteSummary> => {
  const results: CaptureTrustCaseResult[] = [];

  for (const testCase of cases) {
    results.push(await executeCase(testCase));
  }

  const passed = results.filter((result) => result.passed).length;
  const failed = results.length - passed;
  const traces = results
    .map((result) => result.trace)
    .filter((trace): trace is CaptureTraceRecord => trace !== null);
  const latency = summarizeCaptureTraces(traces);
  const percentage = results.length === 0 ? 100 : (passed / results.length) * 100;

  return {
    total: results.length,
    passed,
    failed,
    percentage,
    trustScore: percentage,
    latency,
    results,
  };
};

export const formatCaptureTrustSummary = (summary: CaptureTrustSuiteSummary) => {
  const lines = [
    `Capture trust score: ${summary.trustScore.toFixed(1)}% (${summary.passed}/${summary.total})`,
    `Average db write: ${summary.latency.averageDbWriteMs.toFixed(2)}ms | max ${summary.latency.maxDbWriteMs.toFixed(2)}ms`,
    `Average search visible: ${summary.latency.averageSearchVisibleMs.toFixed(2)}ms | max ${summary.latency.maxSearchVisibleMs.toFixed(2)}ms`,
    `Average full propagation: ${summary.latency.averageFullPropagationMs.toFixed(2)}ms | max ${summary.latency.maxFullPropagationMs.toFixed(2)}ms`,
    "",
  ];

  for (const result of summary.results) {
    const header = `${result.passed ? "PASS" : "FAIL"} ${result.label}`;
    lines.push(header);
    lines.push(`  ${result.message}`);

    if (result.trace) {
      const { durations } = result.trace;
      lines.push(
        `  trace=${result.trace.traceId} capture=${result.trace.captureId ?? "pending"} save=${durations.saveDurationMs?.toFixed(2) ?? "n/a"}ms search=${durations.searchVisibilityLatencyMs?.toFixed(2) ?? "n/a"}ms full=${durations.fullPropagationLatencyMs?.toFixed(2) ?? "n/a"}ms`,
      );
    }

    for (const queryResult of result.queryResults) {
      lines.push(
        `  - ${queryResult.label}: rank ${queryResult.rank ?? "miss"} / ${queryResult.maxRank} for "${queryResult.query}"`,
      );
    }

    lines.push("");
  }

  return lines.join("\n").trim();
};
