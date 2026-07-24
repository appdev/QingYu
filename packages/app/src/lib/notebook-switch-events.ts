import { getAppRuntime } from "../runtime";

export type NotebookSwitchRequest = {
  path?: string;
  source: "file-menu" | "native-open" | "recent" | "settings" | "welcome";
};

const notebookSwitchRequestedEvent = "qingyu://notebook-switch-requested";
const notebookSwitchSources = new Set<NotebookSwitchRequest["source"]>([
  "file-menu",
  "native-open",
  "recent",
  "settings",
  "welcome"
]);

function normalizeNotebookSwitchRequest(value: unknown): NotebookSwitchRequest | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<NotebookSwitchRequest>;
  if (!candidate.source || !notebookSwitchSources.has(candidate.source)) return null;
  if (candidate.path !== undefined && (
    typeof candidate.path !== "string" ||
    candidate.path.length === 0 ||
    candidate.path.includes("\0")
  )) return null;
  return {
    ...(candidate.path === undefined ? {} : { path: candidate.path }),
    source: candidate.source
  };
}

export async function emitNotebookSwitchRequested(request: NotebookSwitchRequest) {
  const normalized = normalizeNotebookSwitchRequest(request);
  if (!normalized) return false;
  await getAppRuntime().events.emit(notebookSwitchRequestedEvent, normalized);
  return true;
}

export async function requestPrimaryNotebookSwitch(request: NotebookSwitchRequest) {
  const normalized = normalizeNotebookSwitchRequest(request);
  if (!normalized) return false;
  const runtime = getAppRuntime();
  let path = normalized.path;
  if (path === undefined) {
    const selected = await runtime.files.openMarkdownFolder();
    if (!selected) return false;
    path = selected.path;
  }
  if (runtime.files.requestPrimaryNotebookSwitch) {
    await runtime.files.requestPrimaryNotebookSwitch(path);
    return true;
  }
  await runtime.events.emit(notebookSwitchRequestedEvent, { ...normalized, path });
  return true;
}

export function listenNotebookSwitchRequested(
  onRequest: (request: NotebookSwitchRequest) => unknown
) {
  return getAppRuntime().events.listen<unknown>(notebookSwitchRequestedEvent, (event) => {
    const request = normalizeNotebookSwitchRequest(event.payload);
    if (request) onRequest(request);
  });
}
