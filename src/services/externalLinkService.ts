import { openUrl } from "@tauri-apps/plugin-opener";

export const RECALL_TRIAL_KEY_URL = "https://sidbuilds.com/recall";

export async function openTrialKeyPage() {
  await openUrl(RECALL_TRIAL_KEY_URL);
}
