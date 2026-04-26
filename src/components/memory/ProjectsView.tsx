import { useState } from "react";
import { FolderOpen, Plus, Pencil, Trash, Check, X } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { formatRelativeTimestamp } from "@/domain/formatters";
import type { Project } from "@/domain/types";
import type { MainView } from "@/windows/MainWindow";

export function ProjectsView({ setView }: { setView: (v: MainView) => void }) {
  const [creating, setCreating] = useState(false);
  const [newName,  setNewName]  = useState("");
  const [newDesc,  setNewDesc]  = useState("");
  const [editId,   setEditId]   = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");

  const { projects, create, update, remove, setActiveProject } = useProjectStore();
  const { memories } = useMemoryStore();

  async function handleCreate() {
    if (!newName.trim()) return;
    await create(newName.trim(), newDesc.trim() || null);
    setNewName(""); setNewDesc(""); setCreating(false);
  }

  async function handleUpdate() {
    if (!editId || !editName.trim()) return;
    await update(editId, editName.trim(), editDesc.trim() || null);
    setEditId(null);
  }

  return (
    <div style={{ flex: 1, overflowY: "auto", padding: "44px 52px" }}>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", marginBottom: 32 }}>
        <div>
          <div className="eyebrow" style={{ marginBottom: 3 }}>Organize</div>
          <h1 style={{ fontSize: 26, fontWeight: 700, color: "var(--text-primary)", letterSpacing: "-0.02em" }}>Projects</h1>
          <div className="accent-line" />
        </div>
        <button className="btn-primary" onClick={() => setCreating(true)}>
          <Plus size={14} /> New Project
        </button>
      </div>

      {/* Create form */}
      {creating && (
        <div style={{ background: "var(--surface-2)", border: "1px solid var(--blue-border)", borderRadius: 16, padding: "18px 22px", marginBottom: 20 }}>
          <div className="eyebrow" style={{ marginBottom: 12 }}>New Project</div>
          <input autoFocus value={newName} onChange={e => setNewName(e.target.value)} onKeyDown={e => { if (e.key === "Enter") void handleCreate(); if (e.key === "Escape") setCreating(false); }} placeholder="Project name" className="r-input" style={{ marginBottom: 10 }} />
          <input value={newDesc} onChange={e => setNewDesc(e.target.value)} placeholder="Description (optional)" className="r-input" style={{ marginBottom: 14, color: "var(--text-secondary)" }} />
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn-primary" onClick={handleCreate}>Create</button>
            <button className="btn-ghost" onClick={() => setCreating(false)}>Cancel</button>
          </div>
        </div>
      )}

      {/* Grid */}
      {projects.length === 0 ? (
        <div style={{ textAlign: "center", padding: "72px 32px", border: "1px dashed var(--line)", borderRadius: 20 }}>
          <FolderOpen size={28} color="var(--t-4)" style={{ marginBottom: 14 }} />
          <div style={{ fontSize: 14, color: "var(--t-4)" }}>No projects yet</div>
        </div>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(270px, 1fr))", gap: 16 }}>
          {projects.map(p => {
            const count = memories.filter(m => m.projectId === p.id).length;
            const editing = editId === p.id;

            return (
              <ProjectCard
                key={p.id}
                project={p}
                count={count}
                editing={editing}
                editName={editing ? editName : ""}
                editDesc={editing ? editDesc : ""}
                onSetEditName={setEditName}
                onSetEditDesc={setEditDesc}
                onOpenEdit={() => { setEditId(p.id); setEditName(p.name); setEditDesc(p.description || ""); }}
                onSaveEdit={handleUpdate}
                onCancelEdit={() => setEditId(null)}
                onDelete={() => { if (confirm("Delete project?")) void remove(p.id); }}
                onClick={() => { if (!editing) { setActiveProject(p.id); setView("memories"); } }}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

function ProjectCard({
  project, count, editing,
  editName, editDesc,
  onSetEditName, onSetEditDesc,
  onOpenEdit, onSaveEdit, onCancelEdit,
  onDelete, onClick,
}: {
  project: Project;
  count: number;
  editing: boolean;
  editName: string;
  editDesc: string;
  onSetEditName: (v: string) => void;
  onSetEditDesc: (v: string) => void;
  onOpenEdit: () => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
  onDelete: () => void;
  onClick: () => void;
}) {
  return (
    <div
      style={{ background: "var(--surface-2)", border: "1px solid var(--border-default)", borderRadius: 20, padding: "22px", cursor: editing ? "default" : "pointer", transition: "border-color 150ms, transform 150ms" }}
      onMouseEnter={e => { if (!editing) { e.currentTarget.style.borderColor = "var(--border-strong)"; e.currentTarget.style.transform = "translateY(-2px)"; }}}
      onMouseLeave={e => { e.currentTarget.style.borderColor = "var(--border-default)"; e.currentTarget.style.transform = "translateY(0)"; }}
      onClick={onClick}
    >
      <div style={{ width: 38, height: 38, borderRadius: 9, background: "var(--blue-dim)", border: "1px solid var(--blue-border)", display: "flex", alignItems: "center", justifyContent: "center", marginBottom: 14 }}>
        <FolderOpen size={17} color="var(--blue)" strokeWidth={1.8} />
      </div>

      {editing ? (
        <div onClick={e => e.stopPropagation()}>
          <input autoFocus value={editName} onChange={e => onSetEditName(e.target.value)} className="r-input" style={{ marginBottom: 8, fontSize: 14 }} />
          <input value={editDesc} onChange={e => onSetEditDesc(e.target.value)} placeholder="Description" className="r-input" style={{ marginBottom: 12, fontSize: 13 }} />
          <div style={{ display: "flex", gap: 6 }}>
            <button className="btn-primary" onClick={onSaveEdit} style={{ padding: "6px 14px", fontSize: 13 }}><Check size={12} /> Save</button>
            <button className="btn-ghost" onClick={onCancelEdit} style={{ padding: "6px 11px", fontSize: 13 }}><X size={12} /> Cancel</button>
          </div>
        </div>
      ) : (
        <>
          <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)", marginBottom: 5 }}>{project.name}</div>
          {project.description && <div style={{ fontSize: 13, color: "var(--text-muted)", lineHeight: 1.5, marginBottom: 14 }}>{project.description}</div>}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginTop: project.description ? 0 : 14 }}>
            <span style={{ fontSize: 11, color: "var(--t-4)" }}>
              {count} memor{count !== 1 ? "ies" : "y"} · {formatRelativeTimestamp(project.updatedAt)}
            </span>
            <div style={{ display: "flex", gap: 3 }}>
              <IconBtn onClick={e => { e.stopPropagation(); onOpenEdit(); }}><Pencil size={11} /></IconBtn>
              <IconBtn onClick={e => { e.stopPropagation(); onDelete(); }} danger><Trash size={11} /></IconBtn>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function IconBtn({ onClick, danger, children }: { onClick: React.MouseEventHandler; danger?: boolean; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      style={{
        width: 26, height: 26, borderRadius: 6,
        display: "flex", alignItems: "center", justifyContent: "center",
        color: danger ? "rgba(248,113,113,0.5)" : "var(--t-4)",
        background: "none", border: "none", cursor: "pointer",
        transition: "all 100ms",
      }}
      onMouseEnter={e => {
        (e.currentTarget as HTMLElement).style.background = danger ? "rgba(248,113,113,0.1)" : "rgba(255,255,255,0.06)";
        (e.currentTarget as HTMLElement).style.color = danger ? "var(--danger)" : "var(--t-2)";
      }}
      onMouseLeave={e => {
        (e.currentTarget as HTMLElement).style.background = "none";
        (e.currentTarget as HTMLElement).style.color = danger ? "rgba(248,113,113,0.5)" : "var(--t-4)";
      }}
    >
      {children}
    </button>
  );
}
