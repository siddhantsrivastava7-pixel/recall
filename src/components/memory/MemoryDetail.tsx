import {
  Check,
  Clock3,
  Copy,
  FolderOpen,
  ExternalLink,
  Save,
  Trash,
  X,
} from "lucide-react";
import {
  type CSSProperties,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import {
  formatLongTimestamp,
  formatUrlForDisplay,
  getMemoryDetailReadingText,
  getMemoryDetailSourceLabel,
  getMemoryDisplayPreview,
  getMemoryDisplayProject,
  getMemoryDisplayTitle,
  hasMeaningfulMemoryPreview,
  normalizeReadingText,
  formatRelativeTimestamp,
} from "@/domain/formatters";
import type { Memory } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";
import { getRelatedMemories } from "@/services/context/ContextEngine";
import { openExternalLink } from "@/services/externalLinkService";
import {
  formatResurfaceLabel,
  fromDatetimeLocalValue,
  getResurfacePresetDate,
  isMemoryDueForResurface,
  toDatetimeLocalValue,
} from "@/services/resurface/memoryResurface";
import { useContextStore } from "@/stores/contextStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";

export function MemoryDetail({
  memory,
  onClose,
}: {
  memory: Memory;
  onClose: () => void;
}) {
  const [activeMemoryId, setActiveMemoryId] = useState(memory.id);
  const liveMemory = useMemoryStore((state) =>
    state.memories.find((item) => item.id === activeMemoryId),
  );
  const memories = useMemoryStore((state) => state.memories);
  const currentMemory = liveMemory ?? memory;

  const { update, remove, markOpened } = useMemoryStore();
  const recordMemoryOpened = useContextStore((state) => state.recordMemoryOpened);
  const { projects } = useProjectStore();

  const [titleDraft, setTitleDraft] = useState(currentMemory.title ?? "");
  const [contentDraft, setContentDraft] = useState(currentMemory.content);
  const [noteDraft, setNoteDraft] = useState(currentMemory.note ?? "");
  const [projectIdDraft, setProjectIdDraft] = useState(currentMemory.projectId ?? "");
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [editingTitle, setEditingTitle] = useState(false);
  const [editingContent, setEditingContent] = useState(false);
  const [editingNote, setEditingNote] = useState(false);
  const [movingProject, setMovingProject] = useState(false);
  const [bringBackOpen, setBringBackOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const bodyRef = useRef<HTMLDivElement>(null);
  const titleInputRef = useRef<HTMLInputElement>(null);
  const contentTextareaRef = useRef<HTMLTextAreaElement>(null);
  const noteTextareaRef = useRef<HTMLTextAreaElement>(null);
  const scrollTopRef = useRef(0);
  const loadedMemoryIdRef = useRef(currentMemory.id);
  const openedMemoryIdRef = useRef<string | null>(null);
  const copyResetTimeoutRef = useRef<number | null>(null);

  const generatedTitle = useMemo(
    () =>
      getMemoryDisplayTitle({
        ...currentMemory,
        title: titleDraft || null,
        content: contentDraft,
        note: noteDraft || null,
      }),
    [currentMemory, titleDraft, contentDraft, noteDraft],
  );
  const displayTitle = titleDraft.trim()
    ? getMemoryDisplayTitle({
        ...currentMemory,
        title: titleDraft,
        content: contentDraft,
        note: noteDraft || null,
      })
    : generatedTitle;
  const displayProject = getMemoryDisplayProject(currentMemory);
  const sourceLabel = getMemoryDetailSourceLabel(currentMemory);
  const hasSourceUrl = Boolean(currentMemory.url);
  const isRawUrlContent =
    /^https?:\/\//i.test(contentDraft.trim()) &&
    !/\s/.test(contentDraft.trim());
  const normalizedContent = useMemo(
    () => normalizeReadingText(contentDraft),
    [contentDraft],
  );
  const detailReadingContent = useMemo(() => {
    return getMemoryDetailReadingText({
      ...currentMemory,
      content: contentDraft,
    });
  }, [
    currentMemory,
    contentDraft,
  ]);
  const hasSourcePreview = useMemo(
    () =>
      Boolean(
        currentMemory.url &&
          isRawUrlContent &&
          detailReadingContent !== normalizedContent &&
          hasMeaningfulMemoryPreview({
            ...currentMemory,
            content: contentDraft,
          }),
      ),
    [
      currentMemory,
      contentDraft,
      detailReadingContent,
      isRawUrlContent,
      normalizedContent,
    ],
  );
  const canEditContentInline = !(
    isRawUrlContent && detailReadingContent !== normalizedContent
  );
  const normalizedNote = useMemo(
    () => normalizeReadingText(noteDraft),
    [noteDraft],
  );
  const metadataItems = [
    displayProject,
    sourceLabel,
    formatRelativeTimestamp(currentMemory.updatedAt || currentMemory.createdAt),
  ];
  const resurfaceLabel = formatResurfaceLabel(currentMemory);
  const isDueForResurface = isMemoryDueForResurface(currentMemory);

  useEffect(() => {
    setActiveMemoryId(memory.id);
  }, [memory.id]);
  const relatedMemories = useMemo(
    () =>
      getRelatedMemories(
        currentMemory,
        memories,
        useContextStore.getState().getSessionContext(),
        5,
      ),
    [currentMemory, memories],
  );

  useEffect(() => {
    if (openedMemoryIdRef.current === currentMemory.id) {
      return;
    }

    openedMemoryIdRef.current = currentMemory.id;
    recordMemoryOpened(currentMemory);
    void markOpened(currentMemory.id);
  }, [currentMemory, markOpened, recordMemoryOpened]);

  useEffect(() => {
    if (loadedMemoryIdRef.current === currentMemory.id) {
      return;
    }

    loadedMemoryIdRef.current = currentMemory.id;
    setTitleDraft(currentMemory.title ?? "");
    setContentDraft(currentMemory.content);
    setNoteDraft(currentMemory.note ?? "");
    setProjectIdDraft(currentMemory.projectId ?? "");
    setDirty(false);
    setEditingTitle(false);
    setEditingContent(false);
    setEditingNote(false);
    setMovingProject(false);
    setBringBackOpen(false);
    setCopied(false);
  }, [currentMemory]);

  useEffect(() => {
    setDirty(
      titleDraft !== (currentMemory.title ?? "") ||
        contentDraft !== currentMemory.content ||
        noteDraft !== (currentMemory.note ?? "") ||
        projectIdDraft !== (currentMemory.projectId ?? ""),
    );
  }, [titleDraft, contentDraft, noteDraft, projectIdDraft, currentMemory]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && dirty && !saving) {
        event.preventDefault();
        void save();
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [dirty, saving, titleDraft, contentDraft, noteDraft, projectIdDraft, currentMemory]);

  useEffect(() => {
    if (editingTitle) {
      titleInputRef.current?.focus();
      titleInputRef.current?.select();
    }
  }, [editingTitle]);

  useEffect(() => {
    if (editingContent) {
      contentTextareaRef.current?.focus();
    }
  }, [editingContent]);

  useEffect(() => {
    if (editingNote) {
      noteTextareaRef.current?.focus();
    }
  }, [editingNote]);

  useLayoutEffect(() => {
    if (bodyRef.current) {
      bodyRef.current.scrollTop = scrollTopRef.current;
    }
  }, [editingTitle, editingContent, editingNote, movingProject]);

  useEffect(() => {
    return () => {
      if (copyResetTimeoutRef.current !== null) {
        window.clearTimeout(copyResetTimeoutRef.current);
      }
    };
  }, []);

  function rememberScrollPosition() {
    if (bodyRef.current) {
      scrollTopRef.current = bodyRef.current.scrollTop;
    }
  }

  async function save() {
    if (!contentDraft.trim() || saving) return;

    setSaving(true);
    const result = await update(currentMemory.id, {
      sourceType: currentMemory.sourceType,
      title: titleDraft.trim() || null,
      content: contentDraft,
      note: noteDraft.trim() || null,
      projectId: projectIdDraft || null,
      url: currentMemory.url,
      externalId: currentMemory.externalId,
      folderPath: currentMemory.folderPath,
      sourceApp: currentMemory.sourceApp,
      sourceWindow: currentMemory.sourceWindow,
      createdAt: currentMemory.createdAt,
      updatedAt: null,
    });
    setSaving(false);

    if (!result.ok) {
      return;
    }

    const persisted = useMemoryStore
      .getState()
      .memories.find((item) => item.id === currentMemory.id);
    if (persisted) {
      setTitleDraft(persisted.title ?? "");
      setContentDraft(persisted.content);
      setNoteDraft(persisted.note ?? "");
      setProjectIdDraft(persisted.projectId ?? "");
    }

    setDirty(false);
    setEditingTitle(false);
    setEditingContent(false);
    setEditingNote(false);
    setMovingProject(false);
    setBringBackOpen(false);
  }

  async function copyContent() {
    await tauriClient.writeClipboardText(currentMemory.content);
    setCopied(true);
    if (copyResetTimeoutRef.current !== null) {
      window.clearTimeout(copyResetTimeoutRef.current);
    }
    copyResetTimeoutRef.current = window.setTimeout(() => {
      setCopied(false);
      copyResetTimeoutRef.current = null;
    }, 1200);
  }

  async function openSourceUrl(event?: React.MouseEvent<HTMLElement>) {
    event?.stopPropagation();
    if (!currentMemory.url) return;
    await openExternalLink(currentMemory.url);
  }

  async function deleteMemory() {
    if (!confirm("Delete this memory? This cannot be undone.")) return;
    await remove(currentMemory.id);
    onClose();
  }

  async function setBringBack(iso: string | null) {
    await useMemoryStore.getState().setResurface(currentMemory.id, iso);
    setBringBackOpen(false);
  }

  async function dismissBringBack() {
    await useMemoryStore.getState().dismissResurface(currentMemory.id);
    setBringBackOpen(false);
  }

  function requestClose() {
    if (dirty && !saving && !confirm("Discard unsaved changes?")) {
      return;
    }
    onClose();
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.62)",
        backdropFilter: "blur(4px)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "4vh",
        zIndex: 50,
      }}
      className="anim-fadein"
      onClick={requestClose}
    >
      <div
        style={{
          background: "var(--surface-2)",
          border: "1px solid rgba(255,255,255,0.08)",
          borderRadius: 26,
          width: "100%",
          maxWidth: 860,
          maxHeight: "90vh",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          margin: "0 16px",
        }}
        className="anim-scalein"
        onClick={(event) => event.stopPropagation()}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            padding: "18px 22px 14px",
            borderBottom: "1px solid rgba(255,255,255,0.06)",
          }}
        >
          <div
            style={{
              fontSize: 12,
              color: "rgba(255,255,255,0.26)",
              letterSpacing: "0.06em",
              textTransform: "uppercase",
            }}
          >
            Memory
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {dirty && (
              <button
                className="btn-primary"
                onClick={() => void save()}
                disabled={saving}
                style={{ padding: "8px 16px", fontSize: 13 }}
              >
                <Save size={13} />
                {saving ? "Saving..." : "Save"}
              </button>
            )}

            <HeaderAction
              label={copied ? "Copied" : "Copy"}
              onClick={() => void copyContent()}
              active={copied}
            >
              {copied ? (
                <Check size={13} strokeWidth={2} />
              ) : (
                <Copy size={13} strokeWidth={1.9} />
              )}
            </HeaderAction>

            {currentMemory.url && (
              <HeaderAction label="Open source" onClick={() => void openSourceUrl()}>
                <ExternalLink size={13} strokeWidth={1.9} />
              </HeaderAction>
            )}

            <HeaderAction
              label={bringBackOpen ? "Done" : "Bring back"}
              onClick={() => setBringBackOpen((value) => !value)}
              active={bringBackOpen || Boolean(currentMemory.resurfaceAt)}
            >
              <Clock3 size={13} strokeWidth={1.9} />
            </HeaderAction>

            <HeaderAction
              label={movingProject ? "Done" : "Move"}
              onClick={() => setMovingProject((value) => !value)}
            >
              <FolderOpen size={13} strokeWidth={1.9} />
            </HeaderAction>

            <HeaderAction
              label="Delete"
              danger
              onClick={() => void deleteMemory()}
            >
              <Trash size={13} strokeWidth={1.9} />
            </HeaderAction>

            <button
              className="btn-ghost"
              onClick={requestClose}
              style={{ padding: "7px 10px" }}
            >
              <X size={14} />
            </button>
          </div>
        </div>

        <div
          ref={bodyRef}
          style={{
            flex: 1,
            overflowY: "auto",
            padding: "28px 32px 34px",
          }}
          onScroll={rememberScrollPosition}
        >
          <section style={{ marginBottom: 28 }}>
            {editingTitle ? (
              <input
                ref={titleInputRef}
                value={titleDraft}
                onChange={(event) => setTitleDraft(event.target.value)}
                onBlur={() => setEditingTitle(false)}
                placeholder={generatedTitle}
                style={{
                  width: "100%",
                  background: "transparent",
                  border: "none",
                  outline: "none",
                  fontSize: 32,
                  fontWeight: 680,
                  color: "var(--text-primary)",
                  fontFamily: "inherit",
                  letterSpacing: "-0.03em",
                  lineHeight: 1.15,
                  marginBottom: 12,
                }}
              />
            ) : (
              <button
                onClick={() => setEditingTitle(true)}
                style={{
                  width: "100%",
                  background: "none",
                  border: "none",
                  padding: 0,
                  margin: 0,
                  textAlign: "left",
                  cursor: "text",
                  font: "inherit",
                  marginBottom: 12,
                }}
              >
                <div
                  style={{
                    fontSize: 32,
                    fontWeight: 680,
                    color: "var(--text-primary)",
                    letterSpacing: "-0.03em",
                    lineHeight: 1.15,
                  }}
                >
                  {displayTitle}
                </div>
              </button>
            )}

            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                flexWrap: "wrap",
                fontSize: 13,
                color: "rgba(255,255,255,0.34)",
                lineHeight: 1.6,
              }}
            >
              <MetadataText value={displayProject} />
              <MetadataDivider />
              {hasSourceUrl && currentMemory.url ? (
                <a
                  href={currentMemory.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(event) => {
                    event.preventDefault();
                    void openSourceUrl(event);
                  }}
                  style={{
                    color: "rgba(255,255,255,0.44)",
                    textDecoration: "none",
                  }}
                  title={currentMemory.url}
                  >
                    {sourceLabel}
                  </a>
              ) : (
                <MetadataText value={sourceLabel} />
              )}
              <MetadataDivider />
              <MetadataText
                value={formatRelativeTimestamp(
                  currentMemory.updatedAt || currentMemory.createdAt,
                )}
                title={formatLongTimestamp(currentMemory.updatedAt || currentMemory.createdAt)}
              />
            </div>

            {movingProject && (
              <div style={{ marginTop: 14 }}>
                <select
                  value={projectIdDraft}
                  onChange={(event) => setProjectIdDraft(event.target.value)}
                  style={{
                    background: "rgba(255,255,255,0.04)",
                    border: "1px solid rgba(255,255,255,0.08)",
                    borderRadius: 12,
                    color: "var(--text-primary)",
                    fontSize: 13,
                    fontFamily: "inherit",
                    outline: "none",
                    padding: "10px 12px",
                    minWidth: 220,
                    cursor: "pointer",
                  }}
                >
                  <option value="" style={{ background: "#111827" }}>
                    Inbox
                  </option>
                  {projects.map((project) => (
                    <option
                      key={project.id}
                      value={project.id}
                      style={{ background: "#111827" }}
                    >
                      {project.name}
                    </option>
                  ))}
                </select>
              </div>
            )}

            {bringBackOpen && (
              <div style={{ marginTop: 14 }}>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    flexWrap: "wrap",
                  }}
                >
                  <BringBackPill label="Later today" onClick={() => void setBringBack(getResurfacePresetDate("later_today"))} />
                  <BringBackPill label="Tomorrow" onClick={() => void setBringBack(getResurfacePresetDate("tomorrow"))} />
                  <BringBackPill label="Next week" onClick={() => void setBringBack(getResurfacePresetDate("next_week"))} />
                  <input
                    type="datetime-local"
                    defaultValue={toDatetimeLocalValue(currentMemory.resurfaceAt)}
                    onChange={(event) => void setBringBack(fromDatetimeLocalValue(event.target.value))}
                    style={{
                      background: "rgba(255,255,255,0.04)",
                      border: "1px solid rgba(255,255,255,0.08)",
                      borderRadius: 10,
                      color: "rgba(255,255,255,0.62)",
                      fontSize: 12,
                      fontFamily: "inherit",
                      outline: "none",
                      padding: "8px 10px",
                    }}
                  />
                  {isDueForResurface && (
                    <BringBackPill label="Dismiss" onClick={() => void dismissBringBack()} />
                  )}
                  {currentMemory.resurfaceAt && (
                    <BringBackPill label="Clear" onClick={() => void setBringBack(null)} muted />
                  )}
                </div>
              </div>
            )}

            {resurfaceLabel && !bringBackOpen && (
              <div
                style={{
                  marginTop: 14,
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 7,
                  padding: "6px 10px",
                  borderRadius: 999,
                  background: isDueForResurface ? "var(--blue-dim)" : "rgba(255,255,255,0.04)",
                  border: `1px solid ${isDueForResurface ? "var(--blue-border)" : "rgba(255,255,255,0.06)"}`,
                  color: isDueForResurface ? "var(--blue)" : "rgba(255,255,255,0.42)",
                  fontSize: 12,
                }}
              >
                <Clock3 size={12} strokeWidth={1.9} />
                {resurfaceLabel}
              </div>
            )}
          </section>

          <section style={{ marginBottom: normalizedNote ? 26 : 0 }}>
            {editingContent && canEditContentInline ? (
              <textarea
                ref={contentTextareaRef}
                value={contentDraft}
                onChange={(event) => setContentDraft(event.target.value)}
                onBlur={() => setEditingContent(false)}
                rows={Math.max(10, Math.min(24, normalizeReadingText(contentDraft).split("\n").length + 2))}
                style={editableContentStyle(true)}
              />
            ) : (
              <div
                role={canEditContentInline ? "button" : undefined}
                tabIndex={canEditContentInline ? 0 : undefined}
                onClick={() => {
                  if (canEditContentInline) {
                    setEditingContent(true);
                  }
                }}
                onKeyDown={(event) => {
                  if (!canEditContentInline) return;
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setEditingContent(true);
                  }
                }}
                style={{
                  width: "100%",
                  background: "none",
                  border: "none",
                  padding: 0,
                  margin: 0,
                  textAlign: "left",
                  cursor: canEditContentInline ? "text" : "default",
                  font: "inherit",
                }}
              >
                <div style={editableContentStyle(false)}>
                  {hasSourcePreview && (
                    <div
                      style={{
                        marginBottom: 12,
                        color: "rgba(255,255,255,0.32)",
                        fontSize: 11,
                        fontWeight: 650,
                        letterSpacing: "0.12em",
                        textTransform: "uppercase",
                      }}
                    >
                      Source preview
                    </div>
                  )}
                  <div
                    style={{
                      whiteSpace: "pre-wrap",
                      fontSize: 15,
                      lineHeight: 1.9,
                      color: "rgba(255,255,255,0.86)",
                    }}
                  >
                    {detailReadingContent}
                  </div>
                  {currentMemory.url && (
                    <span
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 8,
                        marginTop: 16,
                        fontSize: 13,
                        lineHeight: 1.5,
                      }}
                    >
                      <a
                        href={currentMemory.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        onClick={(event) => {
                          event.preventDefault();
                          void openSourceUrl(event);
                        }}
                        style={{
                          color: "rgba(255,255,255,0.44)",
                          textDecoration: "none",
                          wordBreak: "break-all",
                        }}
                        title={currentMemory.url}
                      >
                        {formatUrlForDisplay(currentMemory.url, 96)}
                      </a>
                      <button
                        type="button"
                        aria-label="Open source in browser"
                        title="Open source in browser"
                        onClick={(event) => void openSourceUrl(event)}
                        style={{
                          width: 24,
                          height: 24,
                          flexShrink: 0,
                          borderRadius: 8,
                          border: "1px solid rgba(255,255,255,0.06)",
                          background: "rgba(255,255,255,0.04)",
                          color: "rgba(255,255,255,0.46)",
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          cursor: "pointer",
                        }}
                      >
                        <ExternalLink size={12} strokeWidth={1.9} />
                      </button>
                    </span>
                  )}
                </div>
              </div>
            )}
          </section>

          {(normalizedNote || editingNote) && (
            <section>
              <div
                style={{
                  fontSize: 11,
                  color: "rgba(255,255,255,0.26)",
                  letterSpacing: "0.12em",
                  textTransform: "uppercase",
                  marginBottom: 10,
                }}
              >
                Why save this?
              </div>

              {editingNote ? (
                <textarea
                  ref={noteTextareaRef}
                  value={noteDraft}
                  onChange={(event) => setNoteDraft(event.target.value)}
                  onBlur={() => {
                    if (!noteDraft.trim()) {
                      setNoteDraft("");
                    }
                    setEditingNote(false);
                  }}
                  rows={Math.max(3, Math.min(8, normalizeReadingText(noteDraft).split("\n").length + 1))}
                  style={editableNoteStyle(true)}
                />
              ) : (
                <button
                  onClick={() => setEditingNote(true)}
                  style={{
                    width: "100%",
                    background: "none",
                    border: "none",
                    padding: 0,
                    margin: 0,
                    textAlign: "left",
                    cursor: "text",
                    font: "inherit",
                  }}
                >
                  <div style={editableNoteStyle(false)}>
                    <div
                      style={{
                        whiteSpace: "pre-wrap",
                        fontSize: 14,
                        lineHeight: 1.75,
                        color: "rgba(255,255,255,0.68)",
                      }}
                    >
                      {normalizedNote}
                    </div>
                  </div>
                </button>
              )}
            </section>
          )}

          {!normalizedNote && !editingNote && (
            <button
              onClick={() => setEditingNote(true)}
              style={{
                marginTop: 6,
                background: "none",
                border: "none",
                padding: 0,
                color: "rgba(255,255,255,0.34)",
                fontSize: 13,
                fontFamily: "inherit",
                cursor: "pointer",
              }}
            >
              Add a note
            </button>
          )}

          {relatedMemories.length > 0 && (
            <section style={{ marginTop: 34 }}>
              <div
                style={{
                  fontSize: 11,
                  color: "rgba(255,255,255,0.26)",
                  letterSpacing: "0.12em",
                  textTransform: "uppercase",
                  marginBottom: 12,
                }}
              >
                Related
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 10 }}>
                {relatedMemories.map((item) => (
                  <RelatedMemoryButton
                    key={item.memory.id}
                    memory={item.memory}
                    reason={item.reason}
                    onClick={() => {
                      loadedMemoryIdRef.current = "";
                      openedMemoryIdRef.current = null;
                      setActiveMemoryId(item.memory.id);
                    }}
                  />
                ))}
              </div>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

function BringBackPill({
  label,
  onClick,
  muted = false,
}: {
  label: string;
  onClick: () => void;
  muted?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        border: "1px solid rgba(255,255,255,0.07)",
        background: "rgba(255,255,255,0.04)",
        color: muted ? "rgba(255,255,255,0.34)" : "rgba(255,255,255,0.62)",
        borderRadius: 999,
        padding: "8px 11px",
        fontSize: 12,
        fontFamily: "inherit",
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}

function RelatedMemoryButton({
  memory,
  reason,
  onClick,
}: {
  memory: Memory;
  reason: string;
  onClick: () => void;
}) {
  const domain = getMemoryDetailSourceLabel(memory);

  return (
    <button
      onClick={onClick}
      style={{
        background: "rgba(255,255,255,0.025)",
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
          fontSize: 13,
          fontWeight: 620,
          color: "var(--text-primary)",
          lineHeight: 1.4,
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
          color: "rgba(255,255,255,0.42)",
          lineHeight: 1.55,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 2,
          overflow: "hidden",
          marginBottom: 8,
        }}
      >
        {getMemoryDisplayPreview(memory, 100)}
      </div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          fontSize: 11,
          color: "rgba(255,255,255,0.28)",
        }}
      >
        <span>{domain}</span>
        <MetadataDivider />
        <span>{reason}</span>
      </div>
    </button>
  );
}

function HeaderAction({
  children,
  label,
  onClick,
  danger = false,
  active = false,
}: {
  children: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
  active?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "8px 12px",
        background: active ? "var(--blue-dim)" : "rgba(255,255,255,0.04)",
        border: `1px solid ${active ? "var(--blue-border)" : "rgba(255,255,255,0.06)"}`,
        borderRadius: 10,
        color: active
          ? "var(--blue)"
          : danger
            ? "rgba(248,113,113,0.78)"
            : "rgba(255,255,255,0.58)",
        fontSize: 13,
        fontFamily: "inherit",
        cursor: "pointer",
        transition: "background 120ms ease, color 120ms ease, border-color 120ms ease",
      }}
      onMouseEnter={(event) => {
        if (active) {
          event.currentTarget.style.background = "var(--blue-dim)";
          event.currentTarget.style.borderColor = "var(--blue-border)";
          event.currentTarget.style.color = "var(--blue)";
          return;
        }
        event.currentTarget.style.background = danger
          ? "rgba(248,113,113,0.10)"
          : "rgba(255,255,255,0.07)";
        event.currentTarget.style.borderColor = danger
          ? "rgba(248,113,113,0.16)"
          : "rgba(255,255,255,0.09)";
        event.currentTarget.style.color = danger
          ? "var(--danger)"
          : "var(--text-primary)";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.background = active
          ? "var(--blue-dim)"
          : "rgba(255,255,255,0.04)";
        event.currentTarget.style.borderColor = active
          ? "var(--blue-border)"
          : "rgba(255,255,255,0.06)";
        event.currentTarget.style.color = active
          ? "var(--blue)"
          : danger
            ? "rgba(248,113,113,0.78)"
            : "rgba(255,255,255,0.58)";
      }}
    >
      {children}
      {label}
    </button>
  );
}

function MetadataText({
  value,
  title,
}: {
  value: string;
  title?: string;
}) {
  return (
    <span title={title} style={{ color: "rgba(255,255,255,0.34)" }}>
      {value}
    </span>
  );
}

function MetadataDivider() {
  return (
    <span
      style={{
        width: 3,
        height: 3,
        borderRadius: "50%",
        background: "rgba(255,255,255,0.14)",
      }}
    />
  );
}

function editableContentStyle(editing: boolean): CSSProperties {
  return {
    width: "100%",
    minHeight: editing ? 260 : undefined,
    background: editing ? "rgba(255,255,255,0.04)" : "rgba(255,255,255,0.02)",
    border: editing
      ? "1px solid rgba(255,255,255,0.08)"
      : "1px solid rgba(255,255,255,0.04)",
    borderRadius: 18,
    padding: "22px 24px",
    outline: "none",
    resize: "vertical",
    color: "var(--text-primary)",
    fontSize: 15,
    fontFamily: "inherit",
    lineHeight: 1.9,
    transition: "background 140ms ease, border-color 140ms ease",
  };
}

function editableNoteStyle(editing: boolean): CSSProperties {
  return {
    width: "100%",
    background: editing ? "rgba(255,255,255,0.04)" : "rgba(255,255,255,0.02)",
    border: editing
      ? "1px solid rgba(255,255,255,0.08)"
      : "1px solid rgba(255,255,255,0.04)",
    borderRadius: 16,
    padding: "16px 18px",
    outline: "none",
    resize: "vertical",
    color: "rgba(255,255,255,0.74)",
    fontSize: 14,
    fontFamily: "inherit",
    lineHeight: 1.75,
    transition: "background 140ms ease, border-color 140ms ease",
  };
}
