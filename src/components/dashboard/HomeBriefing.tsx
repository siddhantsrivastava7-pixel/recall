import { useMemo } from "react";
import {
  ArrowRight,
  ChevronRight,
  Clock,
  FileText,
  Plus,
  Search,
  Sparkles,
  TrendingUp,
} from "lucide-react";

import type { Memory } from "@/domain/types";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import {
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
  formatRelativeTimestamp,
} from "@/domain/formatters";
import { getDueResurfaceMemories } from "@/services/resurface/memoryResurface";
import { tauriClient } from "@/services/api/tauri-client";
import { ProactiveSurface } from "@/components/dashboard/ProactiveSurface";
import { AiActivityPill } from "@/components/dashboard/AiActivityPill";
import type { MainView } from "@/windows/MainWindow";

interface HomeBriefingProps {
  setView: (view: MainView) => void;
}

/**
 * Daily briefing surface — replaces the generic Dashboard.
 *
 * Every time the user opens Recall, they get a snapshot of themselves:
 * today's transcript summary inline, yesterday's count, top topics this
 * week, on-this-day flashbacks, due-to-revisit queue, and recent memories.
 * All data is computed on-device from the existing memory store — no LLM,
 * no API calls, no remote anything.
 */
export function HomeBriefing({ setView }: HomeBriefingProps) {
  const { memories, selectMemory } = useMemoryStore();
  const { projects } = useProjectStore();

  const todayDailyTranscript = useMemo(
    () => findTodayDailyTranscript(memories),
    [memories],
  );
  const yesterdayCount = useMemo(() => countYesterday(memories), [memories]);
  const yesterdayTopTopic = useMemo(
    () => topTopicForRange(memories, 1, 1),
    [memories],
  );
  const weekCount = useMemo(() => countLastNDays(memories, 7), [memories]);
  const weekTopTopics = useMemo(() => topTopicsForRange(memories, 0, 7, 8), [memories]);
  const monthCount = useMemo(() => countLastNDays(memories, 30), [memories]);
  const flashback = useMemo(() => onThisDayFlashback(memories), [memories]);
  const due = useMemo(() => getDueResurfaceMemories(memories, 4), [memories]);
  const recent = useMemo(() => memories.slice(0, 5), [memories]);

  const greeting = useMemo(() => formatGreetingHeader(), []);

  const openMemory = (memory: Memory) => {
    selectMemory(memory.id);
    setView("memories");
  };

  return (
    <div className="page fade-in">
      <div className="page-head">
        <div className="page-eyebrow">{greeting.eyebrow}</div>
        <h1 className="page-title">{greeting.title}</h1>
        <p className="page-sub">{greeting.sub}</p>
        {/*
          v0.5.28 — AI activity pill. Hidden when AI is happy and
          idle; surfaces only when there's something the user
          should know about (queue paused, jobs failing, OCR
          unavailable, AI master off). Click jumps to AI Settings.
        */}
        <AiActivityPill setView={setView} />
      </div>

      <div className="qs-row">
        <div className="qs-card primary">
          <div className="qs-eyebrow">
            <Plus size={11} strokeWidth={1.7} /> Quick Capture
          </div>
          <h3 className="qs-title">Save a thought</h3>
          <p className="qs-text">
            Open a focused capture window, prefilled with whatever's on your
            clipboard.
          </p>
          <div className="qs-btns">
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void tauriClient.openQuickSaveWindow()}
            >
              <Plus size={13} strokeWidth={1.8} />
              Quick save
              <span
                className="kbd"
                style={{
                  background: "rgba(255,255,255,0.18)",
                  color: "rgba(255,255,255,0.92)",
                }}
              >
                Ctrl+Shift+S
              </span>
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => void tauriClient.openSearchOverlay()}
            >
              <Search size={13} strokeWidth={1.8} />
              Search overlay
              <span className="kbd">Alt+Space</span>
            </button>
          </div>
        </div>
      </div>

      {/*
        v0.5.25 — Proactive surface slot. Sits above the Daily
        recap card so the Weekly recap (or Forgotten gold) is the
        first thing the user sees on Monday morning of a new week.
        Renders ONE card at most, or nothing. Lives inside
        HomeBriefing because that's the actual Home component
        rendered from MainWindow — v0.5.23 mistakenly attached it
        to the unused Dashboard.tsx.
      */}
      <ProactiveSurface setView={setView} />

      {/* Today's transcript summary — inline preview of the daily memory */}
      {todayDailyTranscript ? (
        <DailyTranscriptCard
          memory={todayDailyTranscript}
          onOpen={() => openMemory(todayDailyTranscript)}
        />
      ) : null}

      {/* Stat row — Yesterday / This week / This month */}
      <div className="section-head">
        <div className="section-title">Your week</div>
        <button
          type="button"
          className="btn btn-quiet"
          onClick={() => setView("memories")}
        >
          View all
          <ChevronRight size={12} strokeWidth={1.8} />
        </button>
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))",
          gap: 12,
        }}
      >
        <StatTile
          eyebrow="Yesterday"
          headline={
            yesterdayCount === 0 ? "Nothing yet" : `${yesterdayCount} ${pluralize("memory", "memories", yesterdayCount)}`
          }
          sub={
            yesterdayTopTopic
              ? `Top topic: ${yesterdayTopTopic.token} · ${yesterdayTopTopic.count}`
              : "—"
          }
        />
        <StatTile
          eyebrow="Last 7 days"
          headline={`${weekCount} ${pluralize("memory", "memories", weekCount)}`}
          sub={
            weekTopTopics.length > 0
              ? `Most mentioned: ${weekTopTopics[0].token}`
              : "—"
          }
        />
        <StatTile
          eyebrow="Last 30 days"
          headline={`${monthCount} ${pluralize("memory", "memories", monthCount)}`}
          sub={`${projects.length} ${pluralize("project", "projects", projects.length)} active`}
        />
      </div>

      {/* Top topics — pill list */}
      {weekTopTopics.length > 0 ? (
        <>
          <div className="section-head">
            <div className="section-title">Top topics this week</div>
          </div>
          <div className="pills">
            {weekTopTopics.map((topic) => (
              <button
                key={topic.token}
                type="button"
                className="pill-filter"
                onClick={() => setView("memories")}
                title={`${topic.count} ${pluralize("memory", "memories", topic.count)} mention this`}
              >
                <TrendingUp size={11} strokeWidth={1.8} />
                {topic.token}
                <span style={{ opacity: 0.55, marginLeft: 4 }}>{topic.count}</span>
              </button>
            ))}
          </div>
        </>
      ) : null}

      {/* On this day — flashbacks */}
      {flashback ? (
        <>
          <div className="section-head">
            <div className="section-title">{flashback.label}</div>
            <div className="section-meta">
              {flashback.memories.length}{" "}
              {pluralize("memory", "memories", flashback.memories.length)}
            </div>
          </div>
          <BriefList memories={flashback.memories} onOpen={openMemory} />
        </>
      ) : null}

      {/* Due to revisit */}
      {due.length > 0 ? (
        <>
          <div className="section-head">
            <div className="section-title">Due to revisit</div>
            <div className="section-meta">
              {due.length} {pluralize("memory", "memories", due.length)}
            </div>
          </div>
          <BriefList memories={due} onOpen={openMemory} />
        </>
      ) : null}

      {/* Recent memories */}
      {recent.length > 0 ? (
        <>
          <div className="section-head">
            <div className="section-title">Recent</div>
            <button
              type="button"
              className="btn btn-quiet"
              onClick={() => setView("memories")}
            >
              All memories
              <ChevronRight size={12} strokeWidth={1.8} />
            </button>
          </div>
          <BriefList memories={recent} onOpen={openMemory} />
        </>
      ) : null}
    </div>
  );
}

/* ─────────────────────────────────────────────────────────────────────────
   Tiles
   ───────────────────────────────────────────────────────────────────────── */

function StatTile({
  eyebrow,
  headline,
  sub,
}: {
  eyebrow: string;
  headline: string;
  sub: string;
}) {
  return (
    <div className="qs-card" style={{ minWidth: 0 }}>
      <div className="qs-eyebrow">{eyebrow}</div>
      <div
        className="qs-title"
        style={{ fontSize: 22, lineHeight: 1.15, marginBottom: 6 }}
      >
        {headline}
      </div>
      <div style={{ fontSize: 12, color: "var(--t-3)", lineHeight: 1.5 }}>{sub}</div>
    </div>
  );
}

function DailyTranscriptCard({
  memory,
  onOpen,
}: {
  memory: Memory;
  onOpen: () => void;
}) {
  const summaryLines = extractSummaryLines(memory.content);
  return (
    <>
      <div className="section-head">
        <div className="section-title">
          <Sparkles
            size={11}
            strokeWidth={1.8}
            style={{ marginRight: 6, verticalAlign: -1 }}
          />
          Today's transcript
        </div>
        <button
          type="button"
          className="btn btn-quiet"
          onClick={onOpen}
        >
          Open
          <ChevronRight size={12} strokeWidth={1.8} />
        </button>
      </div>
      <button
        type="button"
        onClick={onOpen}
        className="qs-card primary"
        style={{
          width: "100%",
          textAlign: "left",
          border: "none",
          cursor: "pointer",
          fontFamily: "inherit",
          color: "var(--t-1)",
        }}
      >
        <div className="qs-eyebrow">
          <FileText size={11} strokeWidth={1.7} />
          {memory.title ?? "Daily transcript"}
        </div>
        {summaryLines.length > 0 ? (
          <ul
            style={{
              margin: "8px 0 0",
              paddingLeft: 18,
              fontSize: 13,
              color: "var(--t-2)",
              lineHeight: 1.6,
            }}
          >
            {summaryLines.slice(0, 5).map((line, index) => (
              <li key={index}>{line}</li>
            ))}
          </ul>
        ) : (
          <p className="qs-text" style={{ margin: "8px 0 0" }}>
            Transcript captured today — open to see entries.
          </p>
        )}
      </button>
    </>
  );
}

function BriefList({
  memories,
  onOpen,
}: {
  memories: Memory[];
  onOpen: (memory: Memory) => void;
}) {
  return (
    <div className="mem-list">
      {memories.map((memory) => (
        <BriefRow key={memory.id} memory={memory} onOpen={onOpen} />
      ))}
    </div>
  );
}

function BriefRow({
  memory,
  onOpen,
}: {
  memory: Memory;
  onOpen: (memory: Memory) => void;
}) {
  const title = getMemoryDisplayTitle(memory);
  const preview = getMemoryDisplayPreview(memory, 180);

  return (
    <div className="mem-item" onClick={() => onOpen(memory)}>
      <div className="mem-row">
        <div className="mem-icon">
          <FileText size={14} strokeWidth={1.7} />
        </div>
        <div className="mem-body">
          <div className="mem-title">{title}</div>
          <p className="mem-preview">{preview}</p>
          <div className="mem-meta">
            {memory.projectName ? <span className="mem-tag">{memory.projectName}</span> : null}
            {memory.projectName ? <span className="dot" /> : null}
            <Clock size={10} strokeWidth={1.8} />
            <span>{formatRelativeTimestamp(memory.createdAt)}</span>
          </div>
        </div>
        <div className="mem-actions" style={{ opacity: 0 }}>
          <ArrowRight size={14} strokeWidth={1.7} />
        </div>
      </div>
    </div>
  );
}

/* ─────────────────────────────────────────────────────────────────────────
   Pure helpers — date math + topic frequency. No LLM, no network.
   ───────────────────────────────────────────────────────────────────────── */

const TOPIC_STOPWORDS = new Set<string>([
  "about",
  "after",
  "also",
  "and",
  "any",
  "app",
  "apps",
  "because",
  "been",
  "before",
  "being",
  "but",
  "could",
  "did",
  "does",
  "doing",
  "down",
  "even",
  "every",
  "for",
  "from",
  "have",
  "here",
  "into",
  "just",
  "know",
  "like",
  "look",
  "made",
  "make",
  "maybe",
  "much",
  "need",
  "next",
  "onto",
  "only",
  "other",
  "our",
  "out",
  "over",
  "really",
  "said",
  "same",
  "should",
  "some",
  "something",
  "still",
  "such",
  "than",
  "that",
  "their",
  "them",
  "then",
  "there",
  "these",
  "they",
  "thing",
  "things",
  "think",
  "this",
  "those",
  "through",
  "today",
  "transcript",
  "user",
  "using",
  "want",
  "what",
  "when",
  "where",
  "which",
  "while",
  "with",
  "work",
  "would",
  "yeah",
  "your",
  "you're",
  "you've",
  "i'm",
]);

function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9_-]+/)
    .map((token) => token.trim())
    .filter(
      (token) =>
        token.length >= 4 &&
        !TOPIC_STOPWORDS.has(token) &&
        !/^\d+$/.test(token),
    );
}

function startOfDay(d: Date): Date {
  const copy = new Date(d);
  copy.setHours(0, 0, 0, 0);
  return copy;
}

function daysAgo(n: number): Date {
  const d = startOfDay(new Date());
  d.setDate(d.getDate() - n);
  return d;
}

function isWithinRange(iso: string, startInclusive: Date, endExclusive: Date): boolean {
  const date = new Date(iso);
  return date >= startInclusive && date < endExclusive;
}

function countLastNDays(memories: Memory[], n: number): number {
  const start = daysAgo(n);
  const end = startOfDay(new Date());
  end.setDate(end.getDate() + 1); // include today
  return memories.filter((m) => isWithinRange(m.createdAt, start, end)).length;
}

function countYesterday(memories: Memory[]): number {
  const start = daysAgo(1);
  const end = startOfDay(new Date());
  return memories.filter((m) => isWithinRange(m.createdAt, start, end)).length;
}

interface TopicCount {
  token: string;
  count: number;
}

/** Top topic in the [n_start, n_end) days-ago window. */
function topTopicForRange(
  memories: Memory[],
  nStart: number,
  nEnd: number,
): TopicCount | null {
  const top = topTopicsForRange(memories, nStart, nEnd, 1);
  return top[0] ?? null;
}

function topTopicsForRange(
  memories: Memory[],
  nStartInclusive: number,
  nEndExclusive: number,
  limit: number,
): TopicCount[] {
  const start = daysAgo(nEndExclusive);
  const end = daysAgo(nStartInclusive - 1);
  const scoped = memories.filter((m) => isWithinRange(m.createdAt, start, end));
  const counts = new Map<string, number>();

  for (const memory of scoped) {
    const labels = memory.topicLabels ?? [];
    if (labels.length > 0) {
      // Prefer pre-extracted topic labels when present (cheaper, more curated).
      for (const label of labels) {
        const normalized = label.trim().toLowerCase();
        if (!normalized || TOPIC_STOPWORDS.has(normalized)) continue;
        counts.set(normalized, (counts.get(normalized) ?? 0) + 1);
      }
      continue;
    }
    // Fallback to tokenizing title + content if no topicLabels.
    const tokens = tokenize(`${memory.title ?? ""} ${memory.content}`);
    const seen = new Set<string>();
    for (const token of tokens) {
      if (seen.has(token)) continue;
      seen.add(token);
      counts.set(token, (counts.get(token) ?? 0) + 1);
    }
  }

  return Array.from(counts.entries())
    .filter(([, count]) => count >= 2)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, limit)
    .map(([token, count]) => ({ token, count }));
}

/** Find today's daily-transcript memory if one exists. */
function findTodayDailyTranscript(memories: Memory[]): Memory | null {
  const today = new Date();
  const todayKey = `${today.getFullYear()}-${pad(today.getMonth() + 1)}-${pad(today.getDate())}`;
  const externalIdToday = `spoken-daily:${todayKey}`;
  return (
    memories.find(
      (memory) => memory.externalId === externalIdToday,
    ) ?? null
  );
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** Extract bullet lines from the Summary section of a daily transcript. */
function extractSummaryLines(content: string): string[] {
  const summaryStart = content.indexOf("\nSummary\n");
  if (summaryStart === -1) return [];
  const transcriptStart = content.indexOf("\n\nTranscript\n", summaryStart);
  const summaryBlock = content.slice(
    summaryStart + "\nSummary\n".length,
    transcriptStart === -1 ? content.length : transcriptStart,
  );
  return summaryBlock
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("- "))
    .map((line) => line.slice(2).trim());
}

/**
 * "On this day" flashback — pick a window in the past and surface 1-3
 * memories captured around the same calendar slot. Tries 1 month → 3 months
 * → 1 year, falling back to last week if the user is too new for any of
 * those. Returns null if there's nothing to show.
 */
function onThisDayFlashback(
  memories: Memory[],
): { label: string; memories: Memory[] } | null {
  const now = startOfDay(new Date());
  const candidates: { label: string; from: Date; to: Date }[] = [
    {
      label: "On this day · 1 month ago",
      from: shiftMonths(now, -1, -1),
      to: shiftMonths(now, -1, 1),
    },
    {
      label: "On this day · 3 months ago",
      from: shiftMonths(now, -3, -1),
      to: shiftMonths(now, -3, 1),
    },
    {
      label: "On this day · 1 year ago",
      from: shiftMonths(now, -12, -1),
      to: shiftMonths(now, -12, 1),
    },
    {
      label: "Last week",
      from: daysAgo(8),
      to: daysAgo(6),
    },
  ];

  for (const window of candidates) {
    const matches = memories
      .filter((memory) => isWithinRange(memory.createdAt, window.from, window.to))
      .slice(0, 3);
    if (matches.length > 0) {
      return { label: window.label, memories: matches };
    }
  }
  return null;
}

/** Return a date that is `monthsDelta` months from `base`, plus `dayOffset`
 *  days for the start/end of a +/- 1 day window. */
function shiftMonths(base: Date, monthsDelta: number, dayOffset: number): Date {
  const d = new Date(base);
  d.setMonth(d.getMonth() + monthsDelta);
  d.setDate(d.getDate() + dayOffset);
  d.setHours(0, 0, 0, 0);
  return d;
}

function pluralize(singular: string, plural: string, count: number): string {
  return count === 1 ? singular : plural;
}

function formatGreetingHeader(): { eyebrow: string; title: string; sub: string } {
  const now = new Date();
  const eyebrow = now.toLocaleDateString(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  });
  const hour = now.getHours();
  let title = "Welcome back.";
  if (hour < 5) title = "Still up.";
  else if (hour < 12) title = "Good morning.";
  else if (hour < 17) title = "Good afternoon.";
  else if (hour < 21) title = "Good evening.";
  else title = "Late shift.";
  const sub = "Your local memory layer. Stays on this device.";
  return { eyebrow, title, sub };
}
