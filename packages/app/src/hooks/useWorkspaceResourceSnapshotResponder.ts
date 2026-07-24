import { useEffect, useRef } from "react";
import type { MarkdownDocumentTab } from "./markdown-document/document-model";
import { getAppRuntime } from "../runtime";
import {
  workspaceResourceSnapshotRequestEvent,
  workspaceResourceSnapshotResponseEvent,
  type WorkspaceResourceDirtyDocument,
  type WorkspaceResourceSnapshotRequest,
  type WorkspaceResourceSnapshotResponse
} from "../lib/workspace-resource-snapshots";

type SavedDocumentVersion = {
  content: string;
  deleted: boolean;
  dirty: boolean;
  open: boolean;
  path: string;
  revision: number;
};

type SnapshotResponderState = {
  dirtyDocuments: WorkspaceResourceDirtyDocument[];
  documentGeneration: number;
  workspaceSourcePath: string | null;
};

function sameSavedDocumentVersions(
  current: readonly SavedDocumentVersion[],
  previous: readonly SavedDocumentVersion[]
) {
  if (current.length !== previous.length) return false;

  return current.every((document, index) => {
    const oldDocument = previous[index];
    return oldDocument !== undefined &&
      document.content === oldDocument.content &&
      document.deleted === oldDocument.deleted &&
      document.dirty === oldDocument.dirty &&
      document.open === oldDocument.open &&
      document.path === oldDocument.path &&
      document.revision === oldDocument.revision;
  });
}

function isSnapshotRequest(value: unknown): value is WorkspaceResourceSnapshotRequest {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const request = value as Record<string, unknown>;

  return typeof request.requestId === "string" && request.requestId.length > 0 &&
    typeof request.sourceWindowLabel === "string" && request.sourceWindowLabel.length > 0 &&
    typeof request.workspaceSourcePath === "string" && request.workspaceSourcePath.length > 0;
}

export function useWorkspaceResourceSnapshotResponder(input: {
  documentTabs: readonly MarkdownDocumentTab[];
  workspaceSourcePath: string | null;
}) {
  const generationRef = useRef(0);
  const previousWorkspaceSourcePathRef = useRef<string | null | undefined>(undefined);
  const savedDocumentVersionsRef = useRef<readonly SavedDocumentVersion[]>([]);
  const responderStateRef = useRef<SnapshotResponderState>({
    dirtyDocuments: [],
    documentGeneration: 0,
    workspaceSourcePath: null
  });
  const savedDocumentVersions = input.documentTabs
    .filter((tab): tab is MarkdownDocumentTab & { path: string } => tab.path !== null)
    .map((tab) => ({
      content: tab.content,
      deleted: Boolean(tab.deleted),
      dirty: tab.dirty,
      open: tab.open,
      path: tab.path,
      revision: tab.revision
    }))
    .sort((left, right) => left.path.localeCompare(right.path));
  const relevantStateChanged =
    previousWorkspaceSourcePathRef.current !== input.workspaceSourcePath ||
    !sameSavedDocumentVersions(savedDocumentVersions, savedDocumentVersionsRef.current);

  if (relevantStateChanged) {
    generationRef.current += 1;
    previousWorkspaceSourcePathRef.current = input.workspaceSourcePath;
    savedDocumentVersionsRef.current = savedDocumentVersions;
  }

  responderStateRef.current = {
    dirtyDocuments: savedDocumentVersions
      .filter((document) => document.open && document.dirty && !document.deleted)
      .map(({ content, path, revision }) => ({ content, path, revision })),
    documentGeneration: generationRef.current,
    workspaceSourcePath: input.workspaceSourcePath
  };

  useEffect(() => {
    const runtime = getAppRuntime();
    if (!runtime.events.isAvailable()) return;

    let active = true;
    let stopListening: (() => unknown) | null = null;
    const startListening = async () => {
      const currentWindowLabel = await runtime.window.getCurrentWindowLabel();
      if (!active || !currentWindowLabel) return;

      const cleanup = await runtime.events.listen<unknown>(
        workspaceResourceSnapshotRequestEvent,
        ({ payload }) => {
          if (!active || !isSnapshotRequest(payload)) return;
          const current = responderStateRef.current;
          if (
            payload.sourceWindowLabel !== currentWindowLabel ||
            payload.workspaceSourcePath !== current.workspaceSourcePath
          ) return;

          const response: WorkspaceResourceSnapshotResponse = {
            documentGeneration: current.documentGeneration,
            dirtyDocuments: current.dirtyDocuments,
            requestId: payload.requestId,
            sourceWindowLabel: currentWindowLabel,
            workspaceSourcePath: payload.workspaceSourcePath
          };
          runtime.events.emit(workspaceResourceSnapshotResponseEvent, response).catch(() => {});
        }
      );
      if (!active) {
        cleanup();
        return;
      }
      stopListening = cleanup;
    };

    startListening().catch(() => {});
    return () => {
      active = false;
      stopListening?.();
    };
  }, []);
}
