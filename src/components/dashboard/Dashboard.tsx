import { useMemo, useState } from "react";
import { ArrowRight, FolderOpen, Search } from "lucide-react";

import { MemoryCard } from "@/components/memory/MemoryCard";
import { MemoryDetail } from "@/components/memory/MemoryDetail";
import {
  formatRelativeTimestamp,
  getMemoryDisplayDomain,
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
} from "@/domain/formatters";
import type { Memory } from "@/domain/types";
import {
  getBookmarksRelatedToActiveProject,
  getRecentBookmarks,
  getTopBookmarkDomains,
  getUsefulForgottenBookmarks,
} from "@/services/bookmarks/bookmarkIntelligence";
import { getRecallFeed, summarizeSessionContext } from "@/services/context/ContextEngine";
import { searchMemories } from "@/services/search/searchMemories";
import { useContextStore } from "@/stores/contextStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSettingsStore } from "@/stores/settingsStore";
import type { MainView } from "@/windows/MainWindow";

function getGreeting() {
  const hour = new Date().getHours();
  if (hour < 12) return "Good morning.";
  if (hour < 17) return "Good afternoon.";
  return "Good evening.";
}

export function Dashboard({ setView }: { setView: (view: MainView) => void }) {
  const [detailMemory, setDetailMemory] = useState<Memory | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const { memories } = useMemoryStore();
  const { projects, activeProjectId, setActiveProject } = useProjectStore();
  const shortcuts = useSettingsStore((state) => state.shortcuts);
  const recentQueries = useContextStore((state) => state.recentQueries);
  const recentlyOpenedMemoryIds = useContextStore((state) => state.recentlyOpenedMemoryIds);
  const recentCaptureIds = useContextStore((state) => state.recentCaptureIds);
  const recordQuery = useContextStore((state) => state.recordQuery);

  const recentMemories = useMemo(
    () =>
      memories
        .slice()
        .sort(
          (left, right) =>
            new Date(right.updatedAt || right.createdAt).getTime() -
            new Date(left.updatedAt || left.createdAt).getTime(),
        )
        .slice(0, 8),
    [memories],
  );

  const projectSnapshots = useMemo(
    () =>
      projects
        .map((project) => {
          const projectMemories = memories
            .filter((memory) => memory.projectId === project.id)
            .slice()
            .sort(
              (left, right) =>
                new Date(right.updatedAt || right.createdAt).getTime() -
                new Date(left.updatedAt || left.createdAt).getTime(),
            );
          const latestMemory = projectMemories[0] ?? null;

          return {
            project,
            count: projectMemories.length,
            latestMemory,
          };
        })
        .sort((left, right) => {
          if (right.count !== left.count) return right.count - left.count;
          return (
            new Date(
              right.latestMemory?.updatedAt ||
                right.latestMemory?.createdAt ||
                right.project.updatedAt,
            ).getTime() -
            new Date(
              left.latestMemory?.updatedAt ||
                left.latestMemory?.createdAt ||
                left.project.updatedAt,
            ).getTime()
          );
        })
        .slice(0, 6),
    [memories, projects],
  );

  const bookmarkCount = recentMemories.filter((memory) => memory.sourceType === "bookmark").length;
  const recentBookmarks = useMemo(() => getRecentBookmarks(memories, 4), [memories]);
  const usefulForgottenBookmarks = useMemo(
    () => getUsefulForgottenBookmarks(memories, 4),
    [memories],
  );
  const topBookmarkDomains = useMemo(
    () => getTopBookmarkDomains(memories, 5),
    [memories],
  );
  const relatedBookmarks = useMemo(
    () => getBookmarksRelatedToActiveProject(memories, projects, activeProjectId, 4),
    [memories, projects, activeProjectId],
  );
  const activeProject = useMemo(
    () => projects.find((project) => project.id === activeProjectId) ?? null,
    [projects, activeProjectId],
  );
  const dashboardSearchResults = useMemo(
    () =>
      searchQuery.trim()
        ? searchMemories(memories, projects, {
            text: searchQuery,
            limit: 10,
          })
        : [],
    [memories, projects, searchQuery],
  );
  const searchShortcutLabel =
    shortcuts.find((shortcut) => shortcut.action === "open-search")?.accelerator ?? "Alt+Space";
  const visibleMemories = searchQuery.trim()
    ? dashboardSearchResults.map((result) => result.memory)
    : recentMemories;
  const sessionContext = useMemo(
    () => useContextStore.getState().getSessionContext(),
    [memories, activeProjectId, recentQueries, recentlyOpenedMemoryIds, recentCaptureIds],
  );
  const recallFeed = useMemo(
    () => getRecallFeed(memories, projects, sessionContext),
    [memories, projects, sessionContext],
  );
  const sessionSummary = useMemo(() => summarizeSessionContext(sessionContext), [sessionContext]);

  function handleSearchSubmit(event: React.FormEvent) {
    event.preventDefault();
    recordQuery(searchQuery);
    const firstResult = dashboardSearchResults[0]?.memory;
    if (firstResult) {
      setDetailMemory(firstResult);
    }
  }

  return (
    <div style={{ flex: 1, overflowY: "auto", padding: "44px 52px" }}>
      <section style={{ marginBottom: 40 }}>
        <h1
          style={{
            fontSize: 40,
            fontWeight: 700,
            color: "var(--text-primary)",
            letterSpacing: "-0.025em",
            lineHeight: 1.15,
            marginBottom: 8,
          }}
        >
          {getGreeting()}
        </h1>
        <p style={{ fontSize: 15, color: "var(--text-muted)", marginBottom: 24 }}>
          {recentMemories.length === 0
            ? "Capture something once, and keep it ready to retrieve later."
            : `${recentMemories.length} recent memories ready to revisit, including ${bookmarkCount} bookmark${bookmarkCount === 1 ? "" : "s"}.`}
        </p>

        <form onSubmit={handleSearchSubmit} style={{ maxWidth: 560 }}>
          <div
            style={{
              width: "100%",
              display: "flex",
              alignItems: "center",
              gap: 12,
              background: "var(--surface-2)",
              border: "1px solid var(--border-default)",
              borderRadius: 14,
              padding: "13px 18px",
              transition: "border-color 150ms ease",
            }}
            onMouseEnter={(event) => {
              event.currentTarget.style.borderColor = "rgba(79,124,255,0.35)";
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.borderColor = "var(--border-default)";
            }}
          >
            <Search size={16} color="rgba(255,255,255,0.28)" strokeWidth={1.8} />
            <input
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder="Search memories, bookmarks, projects..."
              style={{
                flex: 1,
                background: "transparent",
                border: "none",
                outline: "none",
                fontSize: 15,
                color: "var(--text-primary)",
                fontFamily: "inherit",
              }}
            />
            <span className="kbd">{searchShortcutLabel}</span>
          </div>
        </form>
      </section>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1.45fr) minmax(280px, 0.85fr)",
          gap: 22,
          alignItems: "start",
        }}
      >
        <section style={{ minWidth: 0 }}>
          <SectionHeader
            eyebrow={searchQuery.trim() ? "Search" : "Recent"}
            title={searchQuery.trim() ? "Search results" : "Latest memories"}
            subtitle={
              searchQuery.trim()
                ? "Relevant memories and bookmarks found from your dashboard search."
                : "Your most recent captures, bookmarks, and notes."
            }
            onViewAll={() => setView("memories")}
          />

          {visibleMemories.length === 0 ? (
            <Empty
              message={
                searchQuery.trim()
                  ? `No results for "${searchQuery}"`
                  : "No memories yet."
              }
              hint={
                searchQuery.trim()
                  ? "Try a broader phrase, domain, project, or title fragment you remember."
                  : "Use quick capture or bookmark sync to start building your recall layer."
              }
            />
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              {visibleMemories.map((memory) => (
                <MemoryCard key={memory.id} memory={memory} onSelect={setDetailMemory} />
              ))}
            </div>
          )}
        </section>

        <section style={{ minWidth: 0 }}>
          <SectionHeader
            eyebrow="Projects"
            title="Project snapshots"
            subtitle="Count, activity, and the latest thing each project is holding."
            onViewAll={() => setView("projects")}
          />

          {projectSnapshots.length === 0 ? (
            <Empty
              message="No projects yet."
              hint="Create a project to group work by topic, client, or stream."
            />
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {projectSnapshots.map(({ project, count, latestMemory }) => (
                <ProjectSnapshotCard
                  key={project.id}
                  name={project.name}
                  count={count}
                  lastActivity={
                    latestMemory
                      ? formatRelativeTimestamp(latestMemory.updatedAt || latestMemory.createdAt)
                      : formatRelativeTimestamp(project.updatedAt)
                  }
                  latestTitle={
                    latestMemory ? getMemoryDisplayTitle(latestMemory) : "No memories yet"
                  }
                  latestPreview={
                    latestMemory ? getMemoryDisplayPreview(latestMemory, 96) : null
                  }
                  onClick={() => {
                    setActiveProject(project.id);
                    setView("memories");
                  }}
                />
              ))}
            </div>
          )}
        </section>
      </div>

      {(recallFeed.usefulAgainNow.length > 0 ||
        recallFeed.relatedFromEarlier.length > 0 ||
        recallFeed.youMightAlsoNeed.length > 0) && (
        <section style={{ marginTop: 38 }}>
          <SectionHeader
            eyebrow="Context"
            title="Recall feed"
            subtitle={
              sessionSummary.topics.length > 0
                ? `Based on ${sessionSummary.topics.slice(0, 3).join(", ")}.`
                : "Useful memories resurfaced from quality, activity, and project context."
            }
          />

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
              gap: 18,
              alignItems: "start",
            }}
          >
            <InsightPanel
              title="You might also need"
              subtitle="Contextually relevant items from this session."
              emptyMessage="Search, open, or capture more to build session context."
            >
              {recallFeed.youMightAlsoNeed.map((item) => (
                <BookmarkInsightRow
                  key={item.memory.id}
                  memory={item.memory}
                  meta={item.reason}
                  onClick={() => setDetailMemory(item.memory)}
                />
              ))}
            </InsightPanel>

            <InsightPanel
              title="Related from earlier"
              subtitle="Things you used before that connect to now."
              emptyMessage="Previously opened related memories will appear here."
            >
              {recallFeed.relatedFromEarlier.map((item) => (
                <BookmarkInsightRow
                  key={item.memory.id}
                  memory={item.memory}
                  meta={item.reason}
                  onClick={() => setDetailMemory(item.memory)}
                />
              ))}
            </InsightPanel>

            <InsightPanel
              title="Useful again now"
              subtitle="High-quality saved items worth resurfacing."
              emptyMessage="High-quality forgotten memories will appear here."
            >
              {recallFeed.usefulAgainNow.map((item) => (
                <BookmarkInsightRow
                  key={item.memory.id}
                  memory={item.memory}
                  meta={item.reason}
                  onClick={() => setDetailMemory(item.memory)}
                />
              ))}
            </InsightPanel>
          </div>
        </section>
      )}

      <section style={{ marginTop: 38 }}>
        <SectionHeader
          eyebrow="Bookmarks"
          title="Bookmark intelligence"
          subtitle="Useful bookmark signals surfaced from enrichment, topics, domains, and quality scoring."
        />

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
            gap: 18,
            alignItems: "start",
          }}
        >
          <InsightPanel
            title="Recently imported"
            subtitle="Fresh bookmarks upgraded into readable memory objects."
            emptyMessage="Import bookmarks to start seeing them here."
          >
            {recentBookmarks.map((memory) => (
              <BookmarkInsightRow
                key={memory.id}
                memory={memory}
                meta={formatRelativeTimestamp(memory.createdAt)}
                onClick={() => setDetailMemory(memory)}
              />
            ))}
          </InsightPanel>

          <InsightPanel
            title="Useful bookmarks you forgot"
            subtitle="Older high-signal bookmarks worth resurfacing."
            emptyMessage="As bookmark quality improves, forgotten gems will show up here."
          >
            {usefulForgottenBookmarks.map((memory) => (
              <BookmarkInsightRow
                key={memory.id}
                memory={memory}
                meta={`Quality ${Math.round(memory.bookmarkQualityScore ?? 0)}`}
                onClick={() => setDetailMemory(memory)}
              />
            ))}
          </InsightPanel>

          <InsightPanel
            title="Top domains"
            subtitle="Domains contributing the most useful bookmark coverage."
            emptyMessage="Top domains appear once bookmarks have been imported."
          >
            {topBookmarkDomains.map((domain) => (
              <DomainInsightRow
                key={domain.domain}
                domain={domain.domain}
                count={domain.count}
                quality={domain.averageQuality}
                latestMemory={domain.latestMemory}
                onOpen={() => setDetailMemory(domain.latestMemory)}
              />
            ))}
          </InsightPanel>
        </div>

        {activeProject && (
          <div style={{ marginTop: 18 }}>
            <InsightPanel
              title={`Related to ${activeProject.name}`}
              subtitle="Bookmarks connected to your currently active project through titles, topics, and folder context."
              emptyMessage="Select a project in Memories or Projects to surface related bookmarks here."
            >
              {relatedBookmarks.map((memory) => (
                <BookmarkInsightRow
                  key={memory.id}
                  memory={memory}
                  meta={getMemoryDisplayDomain(memory) ?? "Bookmark"}
                  onClick={() => setDetailMemory(memory)}
                />
              ))}
            </InsightPanel>
          </div>
        )}
      </section>

      {detailMemory && (
        <MemoryDetail memory={detailMemory} onClose={() => setDetailMemory(null)} />
      )}
    </div>
  );
}

function SectionHeader({
  eyebrow,
  title,
  subtitle,
  onViewAll,
}: {
  eyebrow: string;
  title: string;
  subtitle: string;
  onViewAll?: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-end",
        justifyContent: "space-between",
        gap: 12,
        marginBottom: 18,
      }}
    >
      <div>
        <div className="eyebrow" style={{ marginBottom: 5 }}>
          {eyebrow}
        </div>
        <h2
          style={{
            fontSize: 18,
            fontWeight: 700,
            color: "var(--text-primary)",
            letterSpacing: "-0.01em",
            marginBottom: 5,
          }}
        >
          {title}
        </h2>
        <p style={{ fontSize: 13, color: "var(--text-muted)", lineHeight: 1.55 }}>
          {subtitle}
        </p>
      </div>

      {onViewAll && (
        <button
          onClick={onViewAll}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: 13,
            color: "var(--blue)",
            background: "none",
            border: "none",
            cursor: "pointer",
            fontFamily: "inherit",
            fontWeight: 500,
            flexShrink: 0,
          }}
        >
          View all <ArrowRight size={13} />
        </button>
      )}
    </div>
  );
}

function ProjectSnapshotCard({
  name,
  count,
  lastActivity,
  latestTitle,
  latestPreview,
  onClick,
}: {
  name: string;
  count: number;
  lastActivity: string;
  latestTitle: string;
  latestPreview: string | null;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        background: "var(--surface-2)",
        border: "1px solid var(--border-default)",
        borderRadius: 18,
        padding: "18px 20px",
        textAlign: "left",
        cursor: "pointer",
        fontFamily: "inherit",
        transition: "border-color 150ms ease, transform 150ms ease",
      }}
      onMouseEnter={(event) => {
        event.currentTarget.style.borderColor = "var(--border-strong)";
        event.currentTarget.style.transform = "translateY(-2px)";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.borderColor = "var(--border-default)";
        event.currentTarget.style.transform = "translateY(0)";
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 10,
          marginBottom: 10,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
          <div
            style={{
              width: 30,
              height: 30,
              borderRadius: 9,
              background: "var(--blue-dim)",
              border: "1px solid var(--blue-border)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <FolderOpen size={14} color="var(--blue)" />
          </div>
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                fontSize: 14,
                fontWeight: 600,
                color: "var(--text-primary)",
                marginBottom: 2,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {name}
            </div>
            <div style={{ fontSize: 12, color: "rgba(255,255,255,0.34)" }}>
              {count} memor{count === 1 ? "y" : "ies"}
            </div>
          </div>
        </div>

        <div style={{ fontSize: 12, color: "rgba(255,255,255,0.32)", flexShrink: 0 }}>
          {lastActivity}
        </div>
      </div>

      <div
        style={{
          fontSize: 13,
          color: "var(--text-primary)",
          lineHeight: 1.5,
          marginBottom: latestPreview ? 6 : 0,
          display: "-webkit-box",
          WebkitLineClamp: 1,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {latestTitle}
      </div>
      {latestPreview && (
        <div
          style={{
            fontSize: 12,
            color: "var(--text-muted)",
            lineHeight: 1.55,
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
          }}
        >
          {latestPreview}
        </div>
      )}
    </button>
  );
}

function Empty({ message, hint }: { message: string; hint?: string }) {
  return (
    <div
      style={{
        padding: "36px 28px",
        textAlign: "center",
        border: "1px dashed rgba(255,255,255,0.07)",
        borderRadius: 20,
      }}
    >
      <div style={{ fontSize: 14, color: "rgba(255,255,255,0.34)", marginBottom: 6 }}>
        {message}
      </div>
      {hint && (
        <div style={{ fontSize: 12, color: "rgba(255,255,255,0.20)", lineHeight: 1.55 }}>
          {hint}
        </div>
      )}
    </div>
  );
}

function InsightPanel({
  title,
  subtitle,
  emptyMessage,
  children,
}: {
  title: string;
  subtitle: string;
  emptyMessage: string;
  children: React.ReactNode;
}) {
  const hasChildren = Array.isArray(children) ? children.length > 0 : Boolean(children);

  return (
    <div
      style={{
        background: "var(--surface-2)",
        border: "1px solid var(--border-default)",
        borderRadius: 22,
        padding: "20px 22px",
        minHeight: 240,
      }}
    >
      <div style={{ marginBottom: 16 }}>
        <div
          style={{
            fontSize: 15,
            fontWeight: 650,
            color: "var(--text-primary)",
            letterSpacing: "-0.01em",
            marginBottom: 4,
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.55 }}>
          {subtitle}
        </div>
      </div>

      {hasChildren ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>{children}</div>
      ) : (
        <div style={{ fontSize: 12, color: "rgba(255,255,255,0.24)", lineHeight: 1.6 }}>
          {emptyMessage}
        </div>
      )}
    </div>
  );
}

function BookmarkInsightRow({
  memory,
  meta,
  onClick,
}: {
  memory: Memory;
  meta: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        width: "100%",
        background: "rgba(255,255,255,0.03)",
        border: "1px solid rgba(255,255,255,0.05)",
        borderRadius: 16,
        padding: "14px 15px",
        textAlign: "left",
        cursor: "pointer",
        fontFamily: "inherit",
      }}
    >
      <div
        style={{
          fontSize: 14,
          fontWeight: 600,
          color: "var(--text-primary)",
          lineHeight: 1.45,
          marginBottom: 5,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 1,
          overflow: "hidden",
        }}
      >
        {getMemoryDisplayTitle(memory)}
      </div>
      <div
        style={{
          fontSize: 12,
          color: "var(--text-muted)",
          lineHeight: 1.55,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 2,
          overflow: "hidden",
          marginBottom: 8,
        }}
      >
        {getMemoryDisplayPreview(memory, 90)}
      </div>
      <div style={{ fontSize: 11, color: "rgba(255,255,255,0.26)" }}>{meta}</div>
    </button>
  );
}

function DomainInsightRow({
  domain,
  count,
  quality,
  latestMemory,
  onOpen,
}: {
  domain: string;
  count: number;
  quality: number;
  latestMemory: Memory;
  onOpen: () => void;
}) {
  return (
    <button
      onClick={onOpen}
      style={{
        width: "100%",
        background: "rgba(255,255,255,0.03)",
        border: "1px solid rgba(255,255,255,0.05)",
        borderRadius: 16,
        padding: "14px 15px",
        textAlign: "left",
        cursor: "pointer",
        fontFamily: "inherit",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 10,
          marginBottom: 6,
        }}
      >
        <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>{domain}</div>
        <div style={{ fontSize: 11, color: "rgba(255,255,255,0.26)" }}>
          {count} saved
        </div>
      </div>
      <div
        style={{
          fontSize: 12,
          color: "var(--text-muted)",
          lineHeight: 1.55,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 2,
          overflow: "hidden",
          marginBottom: 8,
        }}
      >
        {getMemoryDisplayTitle(latestMemory)}
      </div>
      <div style={{ fontSize: 11, color: "rgba(255,255,255,0.26)" }}>
        Avg quality {Math.round(quality)}
      </div>
    </button>
  );
}
