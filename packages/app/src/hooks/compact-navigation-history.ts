import type {
  CompactNavigationState,
  CompactPage,
  CompactSettingsCategory
} from "./useCompactNavigation";

type CompactHistoryMarker = {
  sessionId: string;
  stack: CompactNavigationState;
};

type CompactNavigationSessionIdSources = {
  now?: () => number;
  random?: () => number;
  randomUUID?: (() => string) | null;
};

const compactHistoryMarkerKey = "__markraCompactNavigation";
const compactSettingsCategories: readonly CompactSettingsCategory[] = [
  "general",
  "mcp",
  "storage",
  "sync",
  "appearance",
  "editor"
];
let compactNavigationSessionSequence = 0;

function compactPageFromUnknown(value: unknown): CompactPage | null {
  if (typeof value !== "object" || value === null) return null;
  const page = value as Record<string, unknown>;

  if (page.kind === "editor" || page.kind === "files" || page.kind === "settings" || page.kind === "sync-status") {
    return { kind: page.kind };
  }
  if (page.kind === "move-target" && typeof page.path === "string") {
    return { kind: "move-target", path: page.path };
  }
  if (
    page.kind === "settings-detail"
    && typeof page.category === "string"
    && compactSettingsCategories.includes(page.category as CompactSettingsCategory)
  ) {
    return { kind: "settings-detail", category: page.category as CompactSettingsCategory };
  }
  if (
    page.kind === "sync-form"
    && (page.mode === "create" || page.mode === "edit" || page.mode === "recover")
  ) {
    return { kind: "sync-form", mode: page.mode };
  }
  return null;
}

export function compactHistoryState(
  currentState: unknown,
  stack: CompactNavigationState,
  sessionId: string
) {
  const preservedState = typeof currentState === "object" && currentState !== null ? currentState : {};

  return {
    ...preservedState,
    [compactHistoryMarkerKey]: {
      sessionId,
      stack: stack.map((page) => ({ ...page }))
    } satisfies CompactHistoryMarker
  };
}

export function compactStackFromHistoryState(
  state: unknown,
  sessionId: string
): CompactNavigationState | null {
  if (typeof state !== "object" || state === null) return null;
  const marker = (state as Record<string, unknown>)[compactHistoryMarkerKey];
  if (typeof marker !== "object" || marker === null) return null;

  const candidate = marker as Partial<CompactHistoryMarker>;
  if (candidate.sessionId !== sessionId || !Array.isArray(candidate.stack)) return null;

  const stack: CompactPage[] = [];
  for (const value of candidate.stack) {
    const page = compactPageFromUnknown(value);
    if (!page) return null;
    stack.push(page);
  }

  if (stack.length === 0 || stack[0]?.kind !== "editor") return null;
  if (stack.slice(1).some((page) => page.kind === "editor")) return null;
  return stack;
}

export function createCompactNavigationSessionId(
  sources: CompactNavigationSessionIdSources = {}
) {
  compactNavigationSessionSequence += 1;
  const randomUUID = sources.randomUUID === undefined
    ? typeof globalThis.crypto?.randomUUID === "function"
      ? () => globalThis.crypto.randomUUID()
      : null
    : sources.randomUUID;

  try {
    if (randomUUID) return randomUUID();
  } catch {
    // Fall through when a webview exposes randomUUID but does not allow calling it.
  }

  const timestamp = (sources.now ?? Date.now)().toString(36);
  const random = (sources.random ?? Math.random)().toString(36).slice(2);
  return `compact-${timestamp}-${random}-${compactNavigationSessionSequence}`;
}
