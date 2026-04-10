import type { BookmarkBrowser } from "@/domain/types";

export const bookmarkBrowserOptions: Array<{ id: BookmarkBrowser; label: string }> = [
  { id: "chrome", label: "Chrome" },
  { id: "edge", label: "Edge" },
  { id: "brave", label: "Brave" },
];

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
