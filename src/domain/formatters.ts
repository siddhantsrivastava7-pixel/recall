import type { Memory } from "@/domain/types";

const DISPLAY_STOPWORDS = new Set([
  "a",
  "an",
  "and",
  "about",
  "for",
  "from",
  "i",
  "it",
  "of",
  "or",
  "saved",
  "that",
  "the",
  "thing",
  "to",
  "with",
]);

const truncate = (value: string, limit: number) =>
  value.length <= limit ? value : `${value.slice(0, limit).trimEnd()}…`;

const isUrlLike = (value: string) => /^https?:\/\//i.test(value.trim());

const toTitleCase = (value: string) =>
  value.replace(/\b\w/g, (character) => character.toUpperCase());

const normalizeLineBreaks = (value: string) =>
  value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");

export const normalizeReadingText = (value: string | null | undefined) => {
  const lines = normalizeLineBreaks(value ?? "")
    .split("\n")
    .map((line) => line.trim());

  const output: string[] = [];
  let blankCount = 0;

  for (const line of lines) {
    if (!line) {
      blankCount += 1;
      if (blankCount <= 1 && output.length > 0) {
        output.push("");
      }
      continue;
    }

    blankCount = 0;
    output.push(line);
  }

  while (output[0] === "") output.shift();
  while (output[output.length - 1] === "") output.pop();

  return output.join("\n");
};

export const normalizeDisplayText = (value: string | null | undefined) =>
  normalizeLineBreaks(value ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line, index, lines) => line.length > 0 || (index > 0 && lines[index - 1].length > 0))
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();

export const formatRelativeTimestamp = (iso: string): string => {
  const date = new Date(iso);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSecs < 60) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays === 1) return "yesterday";
  if (diffDays < 7) return `${diffDays}d ago`;
  return new Intl.DateTimeFormat("en", { month: "short", day: "numeric" }).format(date);
};

export const formatLongTimestamp = (iso: string): string =>
  new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(iso));

export const formatUrlForDisplay = (url: string, limit = 56): string => {
  const normalized = normalizeDisplayText(url).replace(/^https?:\/\//i, "").replace(/\/$/, "");
  return normalized.length <= limit ? normalized : `${normalized.slice(0, limit)}…`;
};

export const getUrlDomain = (url: string | null | undefined) => {
  const normalized = normalizeDisplayText(url);
  if (!normalized) return null;

  try {
    return new URL(normalized).hostname.replace(/^www\./i, "").toLowerCase();
  } catch {
    const match = normalized.match(/^https?:\/\/([^/]+)/i);
    return match?.[1]?.replace(/^www\./i, "").toLowerCase() ?? null;
  }
};

const getUrlPathDisplay = (url: string | null | undefined, limit = 84) => {
  const normalized = normalizeDisplayText(url);
  if (!normalized) return null;

  try {
    const parsed = new URL(normalized);
    const path = `${parsed.pathname}${parsed.search}${parsed.hash}`.replace(/\/$/, "") || "/";
    return truncate(path, limit);
  } catch {
    return null;
  }
};

const parseUrl = (value: string | null | undefined) => {
  const normalized = normalizeDisplayText(value);
  if (!normalized) return null;

  try {
    return new URL(normalized);
  } catch {
    return null;
  }
};

const humanizeSlug = (value: string | null | undefined) => {
  const normalized = normalizeDisplayText(value);
  if (!normalized) return null;

  const decoded = decodeURIComponent(normalized)
    .replace(/\.[a-z0-9]{2,5}$/i, "")
    .replace(/[_+.-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  if (!decoded || /^[a-z0-9]{6,}$/i.test(decoded)) {
    return null;
  }

  return toTitleCase(decoded);
};

const getUrlSegments = (url: URL) =>
  url.pathname
    .split("/")
    .map((segment) => segment.trim())
    .filter(Boolean);

const getGeneratedLinkTitle = (value: string | null | undefined) => {
  const url = parseUrl(value);
  if (!url) return null;

  const domain = url.hostname.replace(/^www\./i, "").toLowerCase();
  const segments = getUrlSegments(url);

  if (domain === "github.com" && segments.length >= 2) {
    return `GitHub - ${segments[0]}/${segments[1]}`;
  }

  if ((domain === "reddit.com" || domain.endsWith(".reddit.com")) && segments[0] === "r") {
    const subreddit = segments[1];
    const slug = humanizeSlug(segments[4] ?? segments[3]);
    if (subreddit && slug) return `Reddit - r/${subreddit}: ${slug}`;
    if (subreddit) return `Reddit - r/${subreddit}`;
  }

  if (["x.com", "twitter.com", "mobile.twitter.com"].includes(domain)) {
    const handle = segments[0];
    if (handle) return `X post by @${handle}`;
  }

  const lastMeaningfulSegment = [...segments]
    .reverse()
    .map((segment) => humanizeSlug(segment))
    .find(Boolean);

  if (lastMeaningfulSegment) {
    return `${domain} - ${lastMeaningfulSegment}`;
  }

  return domain || null;
};

const getLinkContextFallback = (
  memory: Pick<Memory, "content" | "url" | "domain" | "resolvedDomain">,
) => {
  const rawUrl = memory.url ?? (isUrlLike(memory.content) ? memory.content : null);
  const url = parseUrl(rawUrl);
  if (!url) return null;

  const domain = memory.resolvedDomain ?? memory.domain ?? url.hostname.replace(/^www\./i, "").toLowerCase();
  const segments = getUrlSegments(url);

  if (domain === "github.com" && segments.length >= 2) {
    const pathType = segments[2] ? ` ${segments[2].replace(/-/g, " ")}` : "";
    return `GitHub repository${pathType}: ${segments[0]}/${segments[1]}.`;
  }

  if ((domain === "reddit.com" || domain.endsWith(".reddit.com")) && segments[0] === "r") {
    const subreddit = segments[1];
    const slug = humanizeSlug(segments[4] ?? segments[3]);
    if (subreddit && slug) return `Reddit thread in r/${subreddit}: ${slug}.`;
    if (subreddit) return `Reddit thread in r/${subreddit}.`;
  }

  if (["x.com", "twitter.com", "mobile.twitter.com"].includes(domain)) {
    const handle = segments[0];
    if (handle) {
      return `X post by @${handle}. The full post could not be extracted locally, but the source link is preserved.`;
    }
  }

  const generatedTitle = getGeneratedLinkTitle(rawUrl);
  if (generatedTitle && generatedTitle !== domain) {
    return `${generatedTitle}.`;
  }

  const path = getUrlPathDisplay(rawUrl, 120);
  if (path && path !== "/") {
    return `${domain}${path}`;
  }

  return domain;
};

const isGenericSavedLinkSummary = (value: string | null | undefined) => {
  const normalized = normalizeDisplayText(value).toLowerCase();
  return /^saved link from [a-z0-9.-]+\./.test(normalized);
};

const firstMeaningfulLine = (value: string) =>
  normalizeLineBreaks(value)
    .split("\n")
    .map((line) => normalizeDisplayText(line))
    .find(Boolean) ?? "";

const firstSentence = (value: string) => {
  const normalized = normalizeDisplayText(value);
  const match = normalized.match(/^(.{1,140}?[.!?])(?:\s|$)/);
  return match?.[1]?.trim() ?? "";
};

const isLowSignalDomainTitle = (
  title: string,
  memory: Pick<Memory, "url" | "domain" | "resolvedDomain" | "content">,
) => {
  const normalizedTitle = title.toLowerCase().replace(/^www\./, "");
  const domain = (
    memory.resolvedDomain ??
    memory.domain ??
    getUrlDomain(memory.url ?? memory.content) ??
    ""
  )
    .toLowerCase()
    .replace(/^www\./, "");

  return (
    normalizedTitle === domain ||
    ["x.com", "twitter.com", "mobile.twitter.com"].includes(normalizedTitle) ||
    (normalizedTitle.includes("reddit") &&
      normalizedTitle.includes("please wait") &&
      normalizedTitle.includes("verification")) ||
    (/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(title) && title.split(/\s+/).length === 1)
  );
};

export const getMemoryDisplayProject = (memory: Pick<Memory, "projectName">) =>
  normalizeDisplayText(memory.projectName) || "Inbox";

export const getMemoryDisplaySourceType = (memory: Pick<Memory, "sourceType">) =>
  memory.sourceType === "bookmark" ? "Bookmark" : "Manual";

export const getMemoryDisplaySource = (
  memory: Pick<Memory, "sourceType" | "sourceApp">,
) => {
  const sourceApp = normalizeDisplayText(memory.sourceApp);
  if (sourceApp) return toTitleCase(sourceApp);
  return memory.sourceType === "bookmark" ? "Browser" : "Manual";
};

export const getMemoryDisplayTitle = (
  memory: Pick<
    Memory,
    | "title"
    | "resolvedTitle"
    | "content"
    | "url"
    | "domain"
    | "resolvedDomain"
    | "sourceType"
    | "note"
  >,
) => {
  const explicitTitle = normalizeDisplayText(memory.title);
  const resolvedTitle = normalizeDisplayText(memory.resolvedTitle);
  const urlGeneratedTitle = getGeneratedLinkTitle(memory.url ?? memory.content);

  if (explicitTitle) {
    if (resolvedTitle && isLowSignalDomainTitle(explicitTitle, memory)) {
      return truncate(resolvedTitle, 120);
    }

    if (isLowSignalDomainTitle(explicitTitle, memory) && urlGeneratedTitle) {
      return truncate(urlGeneratedTitle, 120);
    }

    if (isUrlLike(explicitTitle)) {
      if (resolvedTitle) {
        return truncate(resolvedTitle, 120);
      }
      return urlGeneratedTitle ?? getUrlDomain(explicitTitle) ?? memory.resolvedDomain ?? memory.domain ?? explicitTitle;
    }
    return truncate(explicitTitle, 120);
  }

  if (resolvedTitle) {
    return truncate(resolvedTitle, 120);
  }

  const contentLine = firstMeaningfulLine(memory.content);
  if (contentLine) {
    if (isUrlLike(contentLine)) {
      return urlGeneratedTitle ?? memory.resolvedDomain ?? memory.domain ?? getUrlDomain(contentLine) ?? truncate(contentLine, 120);
    }
    return truncate(contentLine, 120);
  }

  const sentence = firstSentence(memory.content);
  if (sentence) return truncate(sentence, 120);

  const noteLine = firstMeaningfulLine(memory.note ?? "");
  if (noteLine) return truncate(noteLine, 120);

  const urlDomain = memory.resolvedDomain ?? memory.domain ?? getUrlDomain(memory.url ?? memory.content);
  if (urlDomain) return urlDomain;

  return "Untitled Memory";
};

export const memoryPreview = (content: string, limit = 160): string => {
  const normalized = normalizeDisplayText(content);
  return truncate(normalized, limit);
};

export const getMemoryDetailReadingText = (
  memory: Pick<
    Memory,
    | "content"
    | "resolvedDescription"
    | "previewText"
    | "summaryText"
    | "extractedText"
    | "url"
    | "domain"
    | "resolvedDomain"
  >,
) => {
  const normalizedContent = normalizeReadingText(memory.content);

  if (!isUrlLike(normalizedContent) || /\s/.test(normalizedContent.trim())) {
    return normalizedContent;
  }

  const richCandidates = [
    normalizeReadingText(memory.extractedText),
    normalizeReadingText(memory.previewText),
    normalizeReadingText(memory.resolvedDescription),
    normalizeReadingText(memory.summaryText),
  ].filter((candidate) => candidate && !isGenericSavedLinkSummary(candidate));

  if (richCandidates.length > 0) {
    return richCandidates[0];
  }

  const genericSummary = normalizeReadingText(memory.summaryText);
  return getLinkContextFallback(memory) || genericSummary || normalizedContent;
};

export const getMemoryDisplayPreview = (
  memory: Pick<
    Memory,
    | "title"
    | "resolvedTitle"
    | "resolvedDescription"
    | "previewText"
    | "summaryText"
    | "extractedText"
    | "resolvedSiteName"
    | "content"
    | "note"
    | "url"
    | "domain"
    | "resolvedDomain"
    | "folderPath"
    | "bookmarkFolderPath"
    | "sourceType"
>,
  limit = 180,
) => {
  const note = normalizeDisplayText(memory.note);
  const summaryText = normalizeDisplayText(memory.summaryText);
  const previewText = normalizeDisplayText(memory.previewText);
  const extractedText = normalizeDisplayText(memory.extractedText);
  const resolvedDescription = normalizeDisplayText(memory.resolvedDescription);
  const resolvedSiteName = normalizeDisplayText(memory.resolvedSiteName);
  const normalizedContent = normalizeDisplayText(memory.content);
  const title = normalizeDisplayText(memory.title);
  const folderPath = normalizeDisplayText(memory.bookmarkFolderPath ?? memory.folderPath);
  const urlDomain = memory.resolvedDomain ?? memory.domain ?? getUrlDomain(memory.url ?? memory.content);
  const urlPath = getUrlPathDisplay(memory.url ?? memory.content, limit);
  const richSourcePreview = [previewText, resolvedDescription, summaryText, extractedText].find(
    (candidate) => candidate && !isGenericSavedLinkSummary(candidate),
  );

  if (memory.sourceType === "bookmark") {
    if (richSourcePreview) return truncate(richSourcePreview, limit);
    if (folderPath) return truncate(folderPath, limit);
    if (note) return truncate(note, limit);
    if (resolvedSiteName && urlDomain) return truncate(`${resolvedSiteName} · ${urlDomain}`, limit);
    if (urlDomain && urlPath && urlPath !== "/") return `${urlDomain}${urlPath}`;
    if (urlDomain) return urlDomain;
  }

  if (richSourcePreview) {
    return truncate(richSourcePreview, limit);
  }

  if (note && normalizedContent.length > 0 && normalizedContent.length < 32) {
    return truncate(`${normalizedContent} ${note}`, limit);
  }

  if (note && isUrlLike(normalizedContent)) {
    return truncate(note, limit);
  }

  if (isUrlLike(normalizedContent)) {
    const linkContext = getLinkContextFallback(memory);
    if (linkContext) return truncate(linkContext, limit);
    if (summaryText) return truncate(summaryText, limit);
  }

  let preview = normalizedContent;

  if (title && preview.toLowerCase().startsWith(title.toLowerCase())) {
    preview = preview.slice(title.length).trim();
  }

  if (!preview && note) preview = note;
  if (!preview && urlPath && urlPath !== "/") preview = urlPath;
  if (!preview && urlDomain) preview = urlDomain;

  return truncate(preview || "Saved to Recall", limit);
};

export const getMemoryDisplayDomain = (
  memory: Pick<Memory, "url" | "domain" | "resolvedDomain" | "content" | "sourceType">,
) => memory.resolvedDomain ?? memory.domain ?? getUrlDomain(memory.url ?? (isUrlLike(memory.content) ? memory.content : null));

export const hasMeaningfulMemoryPreview = (
  memory: Pick<
    Memory,
    | "title"
    | "resolvedTitle"
    | "resolvedDescription"
    | "previewText"
    | "summaryText"
    | "extractedText"
    | "resolvedSiteName"
    | "content"
    | "note"
    | "url"
    | "domain"
    | "resolvedDomain"
    | "folderPath"
    | "bookmarkFolderPath"
    | "sourceType"
  >,
) => {
  const preview = normalizeDisplayText(getMemoryDisplayPreview(memory, 220));
  if (!preview || isGenericSavedLinkSummary(preview) || isUrlLike(preview)) {
    return false;
  }

  const domain = getMemoryDisplayDomain(memory);
  return preview !== domain;
};

export const getMemoryDetailSourceLabel = (
  memory: Pick<Memory, "url" | "domain" | "resolvedDomain" | "content" | "sourceType" | "sourceApp" | "resolvedSiteName">,
) => getMemoryDisplayDomain(memory) ?? getMemoryDisplaySource(memory);

export const getMemoryDisplayNote = (memory: Pick<Memory, "note">) =>
  normalizeReadingText(memory.note);

export const getMemoryDisplayMetadata = (
  memory: Pick<Memory, "projectName" | "sourceType" | "sourceApp" | "createdAt">,
) => [
  getMemoryDisplayProject(memory),
  getMemoryDisplaySource(memory),
  formatRelativeTimestamp(memory.createdAt),
];

export interface HighlightPart {
  text: string;
  matched: boolean;
}

export const getHighlightParts = (text: string, query: string): HighlightPart[] => {
  const value = normalizeDisplayText(text);
  const queryTokens = normalizeDisplayText(query)
    .toLowerCase()
    .split(" ")
    .filter((token) => token.length > 1 && !DISPLAY_STOPWORDS.has(token));

  if (!value || queryTokens.length === 0) {
    return [{ text: value, matched: false }];
  }

  const uniqueTokens = Array.from(new Set(queryTokens)).sort((left, right) => right.length - left.length);
  const pattern = new RegExp(`(${uniqueTokens.map((token) => token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "ig");
  const parts: HighlightPart[] = [];
  let lastIndex = 0;

  for (const match of value.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > lastIndex) {
      parts.push({ text: value.slice(lastIndex, index), matched: false });
    }

    parts.push({ text: match[0], matched: true });
    lastIndex = index + match[0].length;
  }

  if (lastIndex < value.length) {
    parts.push({ text: value.slice(lastIndex), matched: false });
  }

  return parts.filter((part) => part.text.length > 0);
};
