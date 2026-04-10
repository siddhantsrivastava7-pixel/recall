import type { Memory, Project } from "@/domain/types";

const BASE_TIMESTAMP = "2026-04-01T09:00:00.000Z";

const project = (
  id: string,
  name: string,
  description: string | null = null,
): Project => ({
  id,
  name,
  description,
  createdAt: BASE_TIMESTAMP,
  updatedAt: BASE_TIMESTAMP,
});

const memory = ({
  id,
  content,
  ...overrides
}: Partial<Memory> & Pick<Memory, "id" | "content">): Memory => ({
  id,
  sourceType: "manual",
  title: null,
  content,
  note: null,
  projectId: null,
  projectName: null,
  url: null,
  externalId: null,
  folderPath: null,
  sourceApp: null,
  sourceWindow: null,
  createdAt: BASE_TIMESTAMP,
  updatedAt: BASE_TIMESTAMP,
  ...overrides,
});

export interface SearchEvaluationCase {
  id: string;
  query: string;
  expectedTopIds: string[];
  shouldNotAppear?: string[];
  minimumRanks?: Record<string, number>;
  topN?: number;
  note: string;
}

export const rankingFixtureProjects: Project[] = [
  project("project-pricing", "Pricing"),
  project("project-recall-ui", "Recall UI"),
  project("project-research", "Research"),
];

export const rankingFixtureMemories: Memory[] = [
  memory({
    id: "pricing-strategy",
    title: "Pricing strategy",
    content: "A framework for how we position annual plans and rollout discounts.",
    note: "Use this when we talk about pricing architecture.",
    projectId: "project-pricing",
    updatedAt: "2026-04-02T10:00:00.000Z",
  }),
  memory({
    id: "standup-pricing-mention",
    title: "Daily standup",
    content: "We mentioned pricing strategy once while talking about experiments and follow-ups.",
    updatedAt: "2026-04-09T11:30:00.000Z",
  }),
  memory({
    id: "landing-page-hooks",
    title: "Landing page hooks",
    content: "A shortlist of hero headlines and page-open ideas that work fast.",
    note: "Great for homepage rewrites.",
    projectId: "project-recall-ui",
  }),
  memory({
    id: "marketing-notes",
    title: "Marketing notes",
    content: "Landing ideas are strong. The page needs better structure. Hooks can be sharper.",
    updatedAt: "2026-04-08T09:00:00.000Z",
  }),
  memory({
    id: "apple-ui-references",
    title: "Apple UI references",
    content: "Collected examples of calm desktop spacing and restrained visual depth.",
    note: "Useful for Recall shell polish and premium hierarchy.",
    projectId: "project-recall-ui",
  }),
  memory({
    id: "random-apple-article",
    title: "Random article",
    content: "A long post that says apple once and references something unrelated to interface design.",
  }),
  memory({
    id: "animation-timings",
    title: "Animation timings",
    content: "Notes about hover speed, shell motion, and panel transitions.",
    projectId: "project-recall-ui",
  }),
  memory({
    id: "workspace-brief",
    title: "Workspace brief",
    content: "Central workspace summary for shell layout, motion, and navigation.",
    projectId: "project-recall-ui",
  }),
  memory({
    id: "recall-memo",
    title: "Recall memo",
    content: "General UI notes from today about spacing and polish.",
  }),
  memory({
    id: "openai-pricing-docs",
    sourceType: "bookmark",
    title: "OpenAI pricing docs",
    content: "https://platform.openai.com/docs/pricing",
    url: "https://platform.openai.com/docs/pricing",
    folderPath: "Bookmarks Bar / Research / API",
    sourceApp: "chrome",
    externalId: "bookmark-openai-pricing",
  }),
  memory({
    id: "pricing-retrospective",
    title: "Pricing retrospective",
    content: "Docs review for Q2 pricing changes and packaging cleanup.",
    projectId: "project-pricing",
  }),
  memory({
    id: "prompt-engineering-checklist",
    title: "Prompt engineering checklist",
    content: "A repeatable set of prompt review steps for retrieval quality.",
    note: "Good before shipping prompt changes.",
  }),
  memory({
    id: "engineering-retro",
    title: "Engineering retro",
    content: "Checklist for the weekly retro and follow-up owners.",
  }),
  memory({
    id: "investor-pricing-deck",
    title: "Investor pricing deck",
    content: "Deck outline and pricing narrative for the board review.",
    projectId: "project-pricing",
    updatedAt: "2026-04-02T10:00:00.000Z",
  }),
  memory({
    id: "daily-notes-investor",
    title: "Daily notes",
    content: "Pricing came up once during the investor call recap and action items list.",
    updatedAt: "2026-04-09T11:45:00.000Z",
  }),
  memory({
    id: "churn-survey-insight",
    title: "Churn survey insight",
    content: "Survey findings about why people leave after trial activation.",
    note: "Why save this? Strong language for the retention section and homepage messaging.",
    projectId: "project-research",
  }),
  memory({
    id: "customer-interview-synthesis",
    title: "Customer interview synthesis",
    content: "Cross-cutting interview notes about purchase friction and pricing trust.",
    note: "Synthesis from eight customer calls.",
    projectId: "project-research",
  }),
  memory({
    id: "brave-render-docs",
    sourceType: "bookmark",
    title: "Render pipeline docs",
    content: "https://render.com/docs/pipeline-overview",
    url: "https://render.com/docs/pipeline-overview",
    folderPath: "Bookmarks Bar / Render",
    sourceApp: "brave",
    externalId: "bookmark-render-docs",
  }),
  memory({
    id: "q3-roadmap",
    title: "Quarterly roadmap",
    content: "Q3 roadmap for capture, recall, and bookmark ingestion milestones.",
    note: "Prioritize search quality before semantic work.",
  }),
  memory({
    id: "billing-portal-bookmark",
    sourceType: "bookmark",
    title: "Acme billing portal",
    content: "https://billing.acme.com/dashboard",
    url: "https://billing.acme.com/dashboard",
    folderPath: "Bookmarks Bar / Finance",
    sourceApp: "edge",
    externalId: "bookmark-acme-billing",
  }),
  memory({
    id: "token-system-design",
    title: "Render pipeline token system",
    content: "Task switch cost drops when render tokens and pipeline states are named consistently.",
    note: "Useful for design-token architecture conversations.",
  }),
];

export const searchEvaluationCases: SearchEvaluationCase[] = [
  {
    id: "exact-title-phrase",
    query: "pricing strategy",
    expectedTopIds: ["pricing-strategy"],
    minimumRanks: {
      "pricing-strategy": 1,
      "standup-pricing-mention": 3,
    },
    note: "Exact title phrase should outrank a recent body mention.",
  },
  {
    id: "title-prefix",
    query: "pricing strat",
    expectedTopIds: ["pricing-strategy"],
    minimumRanks: {
      "pricing-strategy": 1,
    },
    note: "Prefix matching should still strongly favor the exact title candidate.",
  },
  {
    id: "token-order-flex",
    query: "strategy pricing",
    expectedTopIds: ["pricing-strategy"],
    minimumRanks: {
      "pricing-strategy": 1,
    },
    note: "Exact title tokens should still win even if the query order is reversed.",
  },
  {
    id: "clustered-title",
    query: "landing page hooks",
    expectedTopIds: ["landing-page-hooks"],
    minimumRanks: {
      "landing-page-hooks": 1,
      "marketing-notes": 3,
    },
    note: "Close same-order title tokens should rank highest.",
  },
  {
    id: "short-title-token-query",
    query: "landing hooks",
    expectedTopIds: ["landing-page-hooks"],
    minimumRanks: {
      "landing-page-hooks": 1,
    },
    note: "Compact title-like queries should favor title matches over scattered content terms.",
  },
  {
    id: "filler-phrase",
    query: "that thing i saved about apple ui references",
    expectedTopIds: ["apple-ui-references"],
    minimumRanks: {
      "apple-ui-references": 1,
    },
    shouldNotAppear: ["recall-memo"],
    note: "Safe stopword handling should ignore filler and recover the real target.",
  },
  {
    id: "project-boost",
    query: "recall ui workspace",
    expectedTopIds: ["workspace-brief"],
    minimumRanks: {
      "workspace-brief": 1,
      "apple-ui-references": 3,
    },
    note: "Project-like queries should strongly favor the right project and let content break ties intentionally.",
  },
  {
    id: "bookmark-source-query",
    query: "chrome openai docs",
    expectedTopIds: ["openai-pricing-docs"],
    minimumRanks: {
      "openai-pricing-docs": 1,
    },
    note: "Source-oriented queries should reward browser and URL fields without breaking relevance.",
  },
  {
    id: "bookmark-folder-query",
    query: "bookmarks bar research openai",
    expectedTopIds: ["openai-pricing-docs"],
    minimumRanks: {
      "openai-pricing-docs": 1,
      "billing-portal-bookmark": 5,
    },
    note: "Folder path should help, but the result still needs strong title/URL relevance.",
  },
  {
    id: "fuzzy-typo-title",
    query: "promt enginering checklist",
    expectedTopIds: ["prompt-engineering-checklist"],
    minimumRanks: {
      "prompt-engineering-checklist": 1,
    },
    note: "Light edit-distance typo tolerance should still retrieve the exact intended item.",
  },
  {
    id: "recency-secondary",
    query: "investor pricing deck",
    expectedTopIds: ["investor-pricing-deck"],
    minimumRanks: {
      "investor-pricing-deck": 1,
      "daily-notes-investor": 5,
    },
    shouldNotAppear: ["random-apple-article"],
    note: "Recent weak mentions should not outrank a highly relevant older exact title.",
  },
  {
    id: "note-phrase-boost",
    query: "why save churn survey",
    expectedTopIds: ["churn-survey-insight"],
    minimumRanks: {
      "churn-survey-insight": 1,
    },
    note: "Exact note phrasing should meaningfully boost a result.",
  },
  {
    id: "exact-title-vs-related-body",
    query: "customer interview synthesis",
    expectedTopIds: ["customer-interview-synthesis"],
    minimumRanks: {
      "customer-interview-synthesis": 1,
    },
    note: "Exact title wins over vaguely related pricing documents.",
  },
  {
    id: "content-medium-weight",
    query: "task switch cost render pipeline",
    expectedTopIds: ["token-system-design"],
    minimumRanks: {
      "token-system-design": 1,
      "brave-render-docs": 3,
    },
    note: "Content matches should still retrieve the right note when title is only partial.",
  },
  {
    id: "source-app-boost",
    query: "brave render docs",
    expectedTopIds: ["brave-render-docs"],
    minimumRanks: {
      "brave-render-docs": 1,
    },
    note: "Source app and bookmark metadata should help source-like queries.",
  },
  {
    id: "roadmap-filler",
    query: "saved thing about quarterly roadmap",
    expectedTopIds: ["q3-roadmap"],
    minimumRanks: {
      "q3-roadmap": 1,
    },
    note: "Filler phrases should not stop title retrieval.",
  },
  {
    id: "domain-bookmark",
    query: "acme billing portal",
    expectedTopIds: ["billing-portal-bookmark"],
    minimumRanks: {
      "billing-portal-bookmark": 1,
    },
    shouldNotAppear: ["openai-pricing-docs"],
    note: "Bookmarks should feel like first-class memories during search.",
  },
  {
    id: "token-proximity",
    query: "render pipeline tokens",
    expectedTopIds: ["token-system-design", "brave-render-docs"],
    minimumRanks: {
      "token-system-design": 1,
      "brave-render-docs": 3,
    },
    note: "Token cluster bonuses should help the title/content pair beat looser references.",
  },
];
