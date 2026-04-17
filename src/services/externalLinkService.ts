import { openUrl } from "@tauri-apps/plugin-opener";

export const RECALL_TRIAL_KEY_URL = "https://sidbuilds.com/recall";

export async function openExternalLink(url: string) {
  const normalized = url.trim();
  if (!/^https?:\/\//i.test(normalized)) {
    return;
  }

  await openUrl(normalized);
}

export async function openTrialKeyPage() {
  await openExternalLink(RECALL_TRIAL_KEY_URL);
}
