import { useEffect, useRef } from "react";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import {
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
} from "@/domain/formatters";
import type { Memory } from "@/domain/types";
import { isMemoryDueForResurface } from "@/services/resurface/memoryResurface";
import { useMemoryStore } from "@/stores/memoryStore";

const NOTIFIED_KEY = "recall.resurfaceNotifications.v1";
const CHECK_INTERVAL_MS = 30_000;

const notificationKey = (memory: Pick<Memory, "id" | "resurfaceAt">) =>
  `${memory.id}:${memory.resurfaceAt ?? ""}`;

const readNotifiedKeys = () => {
  try {
    const raw = window.localStorage.getItem(NOTIFIED_KEY);
    const values = raw ? JSON.parse(raw) : [];
    return new Set(
      Array.isArray(values)
        ? values.filter((value) => typeof value === "string")
        : [],
    );
  } catch {
    return new Set<string>();
  }
};

const writeNotifiedKeys = (keys: Set<string>) => {
  try {
    window.localStorage.setItem(
      NOTIFIED_KEY,
      JSON.stringify(Array.from(keys).slice(-300)),
    );
  } catch {
    // Notification dedupe is best-effort only; never block reminder display.
  }
};

const notifyMemoryDue = async (memory: Memory) => {
  const title = getMemoryDisplayTitle(memory);
  const body = getMemoryDisplayPreview(memory, 120);

  sendNotification({
    title: "Recall reminder",
    body: body ? `${title} - ${body}` : title,
  });
};

export const useResurfaceNotifications = () => {
  const memories = useMemoryStore((state) => state.memories);
  const notifiedRef = useRef<Set<string>>(readNotifiedKeys());
  const permissionGrantedRef = useRef<boolean | null>(null);
  const permissionRequestedRef = useRef(false);

  useEffect(() => {
    const ensureNotificationPermission = async () => {
      if (permissionGrantedRef.current !== null) {
        return permissionGrantedRef.current;
      }

      if (await isPermissionGranted()) {
        permissionGrantedRef.current = true;
        return true;
      }

      if (permissionRequestedRef.current) {
        return false;
      }

      permissionRequestedRef.current = true;
      const permission = await requestPermission();
      permissionGrantedRef.current = permission === "granted";
      return permissionGrantedRef.current;
    };

    const checkDueMemories = async () => {
      const dueMemories = memories.filter(isMemoryDueForResurface);
      const pending = dueMemories.filter(
        (memory) => !notifiedRef.current.has(notificationKey(memory)),
      );

      if (pending.length === 0) return;

      const allowed = await ensureNotificationPermission();
      if (!allowed) return;

      for (const memory of pending.slice(0, 3)) {
        const key = notificationKey(memory);
        notifiedRef.current.add(key);
        await notifyMemoryDue(memory);
      }
      writeNotifiedKeys(notifiedRef.current);
    };

    void checkDueMemories();
    const interval = window.setInterval(() => {
      void checkDueMemories();
    }, CHECK_INTERVAL_MS);

    return () => window.clearInterval(interval);
  }, [memories]);
};
