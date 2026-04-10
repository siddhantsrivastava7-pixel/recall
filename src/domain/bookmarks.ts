import type { BookmarkBrowser, RuntimePlatform } from "@/domain/types";

const bookmarkBrowserOptions: Array<{ id: BookmarkBrowser; label: string }> = [
  { id: "chrome", label: "Chrome" },
  { id: "edge", label: "Edge" },
  { id: "brave", label: "Brave" },
  { id: "safari", label: "Safari" },
];

export const getBookmarkBrowserOptions = (platform?: RuntimePlatform | null) =>
  platform === "macos"
    ? bookmarkBrowserOptions
    : bookmarkBrowserOptions.filter((option) => option.id !== "safari");

export const formatBookmarkBrowserLabel = (browser: string | null | undefined) => {
  if (!browser) {
    return "Unavailable";
  }

  const match = bookmarkBrowserOptions.find((option) => option.id === browser);
  if (match) {
    return match.label;
  }

  return browser.charAt(0).toUpperCase() + browser.slice(1);
};
