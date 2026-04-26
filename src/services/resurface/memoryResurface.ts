import type { Memory } from "@/domain/types";

const DAY_MS = 24 * 60 * 60 * 1000;

const nextHour = (date: Date) => {
  const value = new Date(date);
  value.setMinutes(0, 0, 0);
  value.setHours(value.getHours() + 1);
  return value;
};

export const getResurfacePresetDate = (
  preset: "later_today" | "tomorrow" | "next_week",
) => {
  const now = new Date();
  if (preset === "later_today") {
    const target = nextHour(now);
    target.setHours(Math.max(target.getHours(), 17));
    if (target.getTime() - now.getTime() < 60 * 60 * 1000) {
      return new Date(now.getTime() + 3 * 60 * 60 * 1000).toISOString();
    }
    return target.toISOString();
  }

  if (preset === "tomorrow") {
    const target = new Date(now.getTime() + DAY_MS);
    target.setHours(9, 0, 0, 0);
    return target.toISOString();
  }

  const target = new Date(now.getTime() + 7 * DAY_MS);
  target.setHours(9, 0, 0, 0);
  return target.toISOString();
};

export const toDatetimeLocalValue = (iso: string | null | undefined) => {
  if (!iso) return "";
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return "";
  const offsetMs = date.getTimezoneOffset() * 60 * 1000;
  return new Date(date.getTime() - offsetMs).toISOString().slice(0, 16);
};

export const fromDatetimeLocalValue = (value: string) => {
  if (!value) return null;
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return null;
  return date.toISOString();
};

export const isMemoryDueForResurface = (memory: Pick<Memory, "resurfaceAt" | "resurfaceDismissedAt">) => {
  if (!memory.resurfaceAt) return false;
  const dueAt = new Date(memory.resurfaceAt).getTime();
  if (!Number.isFinite(dueAt) || dueAt > Date.now()) return false;
  if (!memory.resurfaceDismissedAt) return true;
  return new Date(memory.resurfaceDismissedAt).getTime() < dueAt;
};

export const formatResurfaceLabel = (memory: Pick<Memory, "resurfaceAt" | "resurfaceDismissedAt">) => {
  if (!memory.resurfaceAt) return null;
  if (isMemoryDueForResurface(memory)) return "Due now";

  const dueAt = new Date(memory.resurfaceAt);
  if (!Number.isFinite(dueAt.getTime())) return null;
  return `Back ${new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(dueAt)}`;
};

export const getDueResurfaceMemories = (memories: Memory[], limit = 6) =>
  memories
    .filter(isMemoryDueForResurface)
    .slice()
    .sort(
      (left, right) =>
        new Date(left.resurfaceAt ?? 0).getTime() - new Date(right.resurfaceAt ?? 0).getTime(),
    )
    .slice(0, limit);
