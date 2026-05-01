import { useMemo, useState } from "react";
import { Search, SlidersHorizontal } from "lucide-react";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useBlendedSearch } from "@/hooks/useBlendedSearch";
import { MemoryCard } from "./MemoryCard";
import { MemoryDetail } from "./MemoryDetail";
import { getProjectRelevantMemories } from "@/services/context/ContextEngine";

export function MemoriesView() {
  const [filter,     setFilter]     = useState("");
  const [sortOrder,  setSortOrder]  = useState<"newest" | "oldest">("newest");

  const { memories, selectedMemoryId, selectMemory } = useMemoryStore();
  const { projects, activeProjectId, setActiveProject } = useProjectStore();
  const projectSuggestions = useMemo(
    () => getProjectRelevantMemories(memories, projects, activeProjectId, 3),
    [memories, projects, activeProjectId],
  );
  const detail = selectedMemoryId
    ? memories.find((memory) => memory.id === selectedMemoryId) ?? null
    : null;

  // v0.3.8: route All Memories filter through the same blended
  // pipeline as the floating-bar search. Empty query keeps the
  // legacy browse-and-sort behavior (project filter + sort order).
  // Non-empty query switches to relevance-ranked results from
  // keyword + async semantic. Project filter applies as a post-hoc
  // intersection so the user's currently-active project still
  // narrows results; sort order is ignored when search is active
  // (relevance dominates).
  const { results: searchResults } = useBlendedSearch(filter, memories, projects, {
    limit: 200,
  });

  let list: typeof memories;
  const isSearching = filter.trim().length > 0;
  if (isSearching) {
    const ranked = searchResults
      .map((r) => r.memory)
      .filter(
        (m) => activeProjectId === "all" || m.projectId === activeProjectId,
      );
    list = ranked;
  } else {
    list = memories.filter(
      (m) => activeProjectId === "all" || m.projectId === activeProjectId,
    );
    if (sortOrder === "oldest") list = [...list].reverse();
  }

  const bookmarkCount = list.filter((memory) => memory.sourceType === "bookmark").length;

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {/* Header bar */}
      <div style={{ padding: "28px 52px 20px", borderBottom: "1px solid rgba(255,255,255,0.05)", flexShrink: 0 }}>
        <div className="eyebrow" style={{ marginBottom: 3 }}>Library</div>
        <h1 style={{ fontSize: 26, fontWeight: 700, color: "var(--text-primary)", letterSpacing: "-0.02em", marginBottom: 18 }}>Memories</h1>

        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          {/* Text filter */}
          <div style={{ display: "flex", alignItems: "center", gap: 9, background: "var(--surface-2)", border: "1px solid var(--border-default)", borderRadius: 10, padding: "9px 13px", maxWidth: 360, flex: "1 1 200px" }}>
            <Search size={14} color="var(--t-4)" strokeWidth={1.8} />
            <input
              value={filter}
              onChange={e => setFilter(e.target.value)}
              placeholder="Filter…"
              style={{ flex: 1, background: "transparent", border: "none", outline: "none", fontSize: 14, color: "var(--text-primary)", fontFamily: "inherit" }}
            />
          </div>

          {/* Project pills */}
          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            <FilterPill label="All" active={activeProjectId === "all"} onClick={() => setActiveProject("all")} />
            {projects.map(p => (
              <FilterPill key={p.id} label={p.name} active={activeProjectId === p.id} onClick={() => setActiveProject(p.id)} />
            ))}
          </div>

          {/* Sort */}
          <button
            onClick={() => setSortOrder(s => s === "newest" ? "oldest" : "newest")}
            style={{ display: "flex", alignItems: "center", gap: 6, padding: "8px 13px", background: "var(--surface-2)", border: "1px solid var(--border-default)", borderRadius: 10, fontSize: 13, color: "var(--text-secondary)", cursor: "pointer", fontFamily: "inherit", marginLeft: "auto" }}
          >
            <SlidersHorizontal size={13} strokeWidth={1.8} />
            {sortOrder === "newest" ? "Newest" : "Oldest"}
          </button>
        </div>
      </div>

      {/* Grid */}
      <div style={{ flex: 1, overflowY: "auto", padding: "24px 52px" }}>
        {list.length === 0 ? (
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: 260, gap: 8 }}>
            <div style={{ fontSize: 14, color: "var(--t-4)" }}>
              {filter ? `No results for "${filter}"` : "No memories yet."}
            </div>
            {!filter && <div style={{ fontSize: 12, color: "var(--t-4)" }}>Press ⌘⇧S to capture your first memory.</div>}
          </div>
        ) : (
          <>
            <div style={{ fontSize: 11, color: "var(--t-4)", marginBottom: 18 }}>
              {list.length} memor{list.length !== 1 ? "ies" : "y"} · {bookmarkCount} bookmark{bookmarkCount === 1 ? "" : "s"}
            </div>
            {projectSuggestions.length > 0 && (
              <div style={{ maxWidth: 920, marginBottom: 24 }}>
                <div className="eyebrow" style={{ marginBottom: 10 }}>
                  Project-relevant
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  {projectSuggestions.map((item) => (
                    <MemoryCard
                      key={item.memory.id}
                      memory={item.memory}
                      resurfaced
                      onSelect={(memory) => selectMemory(memory.id)}
                    />
                  ))}
                </div>
              </div>
            )}
            <div style={{ display: "flex", flexDirection: "column", gap: 14, maxWidth: 920 }}>
              {list.map(m => (
                <MemoryCard
                  key={m.id}
                  memory={m}
                  onSelect={(memory) => selectMemory(memory.id)}
                />
              ))}
            </div>
          </>
        )}
      </div>

      {detail && <MemoryDetail memory={detail} onClose={() => selectMemory(null)} />}
    </div>
  );
}

function FilterPill({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        padding: "6px 13px",
        borderRadius: 8,
        fontSize: 13,
        fontWeight: active ? 600 : 400,
        color: active ? "var(--blue)" : "var(--t-3)",
        background: active ? "var(--blue-dim)" : "transparent",
        border: `1px solid ${active ? "var(--blue-border)" : "var(--border-default)"}`,
        cursor: "pointer",
        fontFamily: "inherit",
        whiteSpace: "nowrap",
        transition: "all 130ms",
      }}
    >
      {label}
    </button>
  );
}
