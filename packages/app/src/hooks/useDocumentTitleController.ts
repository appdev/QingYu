import { useCallback, useEffect, useRef } from "react";
import { readMarkdownFrontmatter, upsertMarkdownFrontmatterTitle } from "@markra/markdown";
import {
  markdownDocumentTitleFromFileName,
  normalizeMarkdownDocumentTitle,
  type AppLanguage
} from "@markra/shared";
import type {
  NativeMarkdownFolderFile,
  SavedNativeMarkdownFile
} from "../lib/tauri";
import type { MarkdownDocumentTab } from "./useMarkdownDocument";

export type DocumentTitleModel = {
  disabled: boolean;
  onCommit: (reason: "blur" | "enter") => unknown;
  onInput: (title: string) => unknown;
  title: string;
};

export type UseDocumentTitleControllerOptions = {
  applyRenamedTreeFile: (previousPath: string, file: NativeMarkdownFolderFile) => unknown;
  handleMarkdownTabChange: (
    tabId: string,
    source: string,
    options: { documentRevision: number }
  ) => unknown;
  isReadOnlyPath: (path: string | null) => boolean;
  language: AppLanguage;
  renameMarkdownTreeFile: (
    file: NativeMarkdownFolderFile,
    fileName: string
  ) => Promise<NativeMarkdownFolderFile | null>;
  saveMarkdownTabContentById: (
    tabId: string,
    source: string,
    options: { skipHistorySnapshot: true }
  ) => Promise<SavedNativeMarkdownFile | null>;
  tabs: readonly MarkdownDocumentTab[];
};

type SourceRequest = {
  sourceAtRequest: string;
  source: string;
};

type TitleTransactionRequest = {
  generation: number;
  kind: "title";
  sourceRequest?: SourceRequest;
  title: string;
};

type RepairTransactionRequest = {
  generation: number;
  kind: "repair";
  sourceRequest?: SourceRequest;
};

type TransactionRequest = RepairTransactionRequest | TitleTransactionRequest;

type QueuedTransaction = {
  request: TransactionRequest;
  waiters: Array<(result: unknown) => unknown>;
};

type TabTransactionState = {
  chain: Promise<unknown>;
  drainQueued: boolean;
  generation: number;
  pendingDraft: TransactionRequest | null;
  queued: QueuedTransaction | null;
  timer: number | null;
};

type RuntimeFileIdentity = {
  file: NativeMarkdownFolderFile;
  previousPath: string;
};

const documentTitleDebounceMilliseconds = 256;

function tabAsFolderFile(tab: MarkdownDocumentTab): NativeMarkdownFolderFile | null {
  if (!tab.path) return null;

  return {
    name: tab.name || "Untitled.md",
    path: tab.path,
    relativePath: tab.path,
    sizeBytes: tab.sizeBytes
  };
}

function latestTab(tabs: readonly MarkdownDocumentTab[], tabId: string) {
  return tabs.find((tab) => tab.id === tabId && tab.open) ?? null;
}

function sourceForRequest(tab: MarkdownDocumentTab, request: TransactionRequest) {
  if (!request.sourceRequest) return tab.content;

  return tab.content === request.sourceRequest.sourceAtRequest
    ? request.sourceRequest.source
    : tab.content;
}

export function useDocumentTitleController(options: UseDocumentTitleControllerOptions) {
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const transactionStatesRef = useRef(new Map<string, TabTransactionState>());
  const authoredSourcesRef = useRef(new Map<string, Set<string>>());
  const runtimeFilesRef = useRef(new Map<string, RuntimeFileIdentity>());

  const transactionState = useCallback((tabId: string) => {
    const existing = transactionStatesRef.current.get(tabId);
    if (existing) return existing;

    const created: TabTransactionState = {
      chain: Promise.resolve(),
      drainQueued: false,
      generation: 0,
      pendingDraft: null,
      queued: null,
      timer: null
    };
    transactionStatesRef.current.set(tabId, created);
    return created;
  }, []);

  const currentFileForTab = useCallback((tab: MarkdownDocumentTab) => {
    const runtimeIdentity = runtimeFilesRef.current.get(tab.id);
    if (!runtimeIdentity) return tabAsFolderFile(tab);

    if (tab.path === runtimeIdentity.file.path || tab.path === runtimeIdentity.previousPath) {
      return runtimeIdentity.file;
    }

    runtimeFilesRef.current.delete(tab.id);
    return tabAsFolderFile(tab);
  }, []);

  const rememberAuthoredSource = useCallback((tabId: string, source: string) => {
    const existing = authoredSourcesRef.current.get(tabId) ?? new Set<string>();
    existing.add(source);
    while (existing.size > 4) {
      const oldest = existing.values().next().value;
      if (typeof oldest !== "string") break;
      existing.delete(oldest);
    }
    authoredSourcesRef.current.set(tabId, existing);
  }, []);

  const routeAndSaveSource = useCallback(async (
    tab: MarkdownDocumentTab,
    source: string
  ) => {
    const currentOptions = optionsRef.current;
    rememberAuthoredSource(tab.id, source);
    currentOptions.handleMarkdownTabChange(tab.id, source, {
      documentRevision: tab.revision
    });
    await currentOptions.saveMarkdownTabContentById(tab.id, source, {
      skipHistorySnapshot: true
    });
  }, [rememberAuthoredSource]);

  const executeTransaction = useCallback(async (
    tabId: string,
    request: TransactionRequest
  ) => {
    const currentOptions = optionsRef.current;
    const tab = latestTab(currentOptions.tabs, tabId);
    if (!tab || currentOptions.isReadOnlyPath(tab.path)) return;
    if (readMarkdownFrontmatter(sourceForRequest(tab, request)).status === "malformed") return;

    let authoritativeFile = currentFileForTab(tab);
    if (!authoritativeFile) return;

    if (request.kind === "title") {
      const normalized = normalizeMarkdownDocumentTitle(request.title);
      if (!normalized.ok) return;

      if (normalized.fileName !== authoritativeFile.name) {
        const previousPath = authoritativeFile.path;
        let renamed: NativeMarkdownFolderFile | null;
        try {
          renamed = await optionsRef.current.renameMarkdownTreeFile(
            authoritativeFile,
            normalized.fileName
          );
        } catch {
          return;
        }
        if (!renamed) return;

        const tabAfterRename = latestTab(optionsRef.current.tabs, tabId);
        if (!tabAfterRename) return;
        const currentIdentity = currentFileForTab(tabAfterRename);
        if (!currentIdentity || currentIdentity.path !== previousPath) return;

        runtimeFilesRef.current.set(tabId, { file: renamed, previousPath });
        optionsRef.current.applyRenamedTreeFile(previousPath, renamed);
        authoritativeFile = renamed;
      }
    }

    const latest = latestTab(optionsRef.current.tabs, tabId);
    if (!latest) return;
    const source = sourceForRequest(latest, request);
    const title = markdownDocumentTitleFromFileName(authoritativeFile.name);
    const patched = upsertMarkdownFrontmatterTitle(source, title);
    if (!patched.ok) return;
    if (request.kind === "repair" && !patched.changed) return;

    await routeAndSaveSource(latest, patched.source);
  }, [currentFileForTab, routeAndSaveSource]);

  const queueTransaction = useCallback((tabId: string, request: TransactionRequest) => {
    const state = transactionState(tabId);
    const completion = new Promise<unknown>((resolve) => {
      state.queued?.waiters.forEach((waiter) => waiter(undefined));
      state.queued = { request, waiters: [resolve] };
    });

    if (!state.drainQueued) {
      state.drainQueued = true;
      state.chain = state.chain
        .catch(() => undefined)
        .then(async () => {
          state.drainQueued = false;
          const queued = state.queued;
          state.queued = null;
          if (!queued) return;

          try {
            if (queued.request.generation === state.generation) {
              await executeTransaction(tabId, queued.request);
            }
          } catch {
            // A routed state or save failure must not reject the per-tab chain.
          } finally {
            queued.waiters.forEach((waiter) => waiter(undefined));
          }
        });
    }

    return completion;
  }, [executeTransaction, transactionState]);

  const clearTimer = useCallback((state: TabTransactionState) => {
    if (state.timer === null) return;
    window.clearTimeout(state.timer);
    state.timer = null;
  }, []);

  const flushDraft = useCallback((tabId: string) => {
    const state = transactionState(tabId);
    clearTimer(state);
    const request = state.pendingDraft;
    state.pendingDraft = null;
    if (!request) return Promise.resolve(undefined);

    return queueTransaction(tabId, request);
  }, [clearTimer, queueTransaction, transactionState]);

  const scheduleTitleRequest = useCallback((
    tabId: string,
    title: string,
    sourceRequest?: SourceRequest
  ) => {
    const state = transactionState(tabId);
    state.generation += 1;
    state.pendingDraft = {
      generation: state.generation,
      kind: "title",
      sourceRequest,
      title
    };
    clearTimer(state);
    state.timer = window.setTimeout(() => {
      state.timer = null;
      const request = state.pendingDraft;
      state.pendingDraft = null;
      if (request) queueTransaction(tabId, request).catch(() => undefined);
    }, documentTitleDebounceMilliseconds);
  }, [clearTimer, queueTransaction, transactionState]);

  const queueRepair = useCallback((tabId: string, sourceRequest?: SourceRequest) => {
    const state = transactionState(tabId);
    state.generation += 1;
    clearTimer(state);
    state.pendingDraft = null;
    return queueTransaction(tabId, {
      generation: state.generation,
      kind: "repair",
      sourceRequest
    });
  }, [clearTimer, queueTransaction, transactionState]);

  const modelForTab = useCallback((tabId: string): DocumentTitleModel | null => {
    const tab = latestTab(optionsRef.current.tabs, tabId);
    if (!tab) return null;

    const file = currentFileForTab(tab);
    const malformed = readMarkdownFrontmatter(tab.content).status === "malformed";
    const disabled = file === null || optionsRef.current.isReadOnlyPath(tab.path) || malformed;

    return {
      disabled,
      onCommit: () => flushDraft(tabId).catch(() => undefined),
      onInput: (title) => {
        const currentTab = latestTab(optionsRef.current.tabs, tabId);
        if (!currentTab) return;
        if (optionsRef.current.isReadOnlyPath(currentTab.path)) return;
        if (readMarkdownFrontmatter(currentTab.content).status === "malformed") return;
        scheduleTitleRequest(tabId, title);
      },
      title: markdownDocumentTitleFromFileName(file?.name ?? tab.name)
    };
  }, [currentFileForTab, flushDraft, scheduleTitleRequest]);

  const handleSourceTitleChange = useCallback((
    tabId: string,
    previousSource: string,
    nextSource: string
  ) => {
    const authoredSources = authoredSourcesRef.current.get(tabId);
    if (authoredSources?.delete(nextSource)) {
      if (authoredSources.size === 0) authoredSourcesRef.current.delete(tabId);
      return true;
    }
    if (authoredSources) authoredSourcesRef.current.delete(tabId);

    const tab = latestTab(optionsRef.current.tabs, tabId);
    if (!tab || optionsRef.current.isReadOnlyPath(tab.path)) return false;

    const previous = readMarkdownFrontmatter(previousSource);
    const next = readMarkdownFrontmatter(nextSource);
    if (next.status === "malformed") return false;

    const sourceRequest = {
      sourceAtRequest: tab.content,
      source: nextSource
    };
    const previousTitle = previous.status === "valid" ? previous.title : null;
    const nextTitle = next.status === "valid" ? next.title : null;

    if (typeof nextTitle === "string" && nextTitle.trim().length > 0) {
      if (previous.status === "valid" && previousTitle === nextTitle) return false;

      scheduleTitleRequest(tabId, nextTitle, sourceRequest);
      return true;
    }

    const removedTitle = typeof previousTitle === "string" && previousTitle.length > 0;
    if (!removedTitle) return false;

    queueRepair(tabId, sourceRequest).catch(() => undefined);
    return true;
  }, [queueRepair, scheduleTitleRequest]);

  const reconcileOpenDocument = useCallback(async (tabId: string) => {
    const tab = latestTab(optionsRef.current.tabs, tabId);
    if (!tab || optionsRef.current.isReadOnlyPath(tab.path)) return undefined;

    const file = currentFileForTab(tab);
    if (!file) return undefined;
    const metadata = readMarkdownFrontmatter(tab.content);
    if (metadata.status === "malformed") return undefined;

    const title = markdownDocumentTitleFromFileName(file.name);
    if (metadata.status === "valid" && metadata.title === title) return undefined;

    await queueRepair(tabId);
    return undefined;
  }, [currentFileForTab, queueRepair]);

  useEffect(() => () => {
    transactionStatesRef.current.forEach((state) => clearTimer(state));
  }, [clearTimer]);

  return {
    handleSourceTitleChange,
    modelForTab,
    reconcileOpenDocument
  };
}
