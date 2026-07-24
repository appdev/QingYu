import {
  parseMarkdownResourceReferences,
  type MarkdownResourceReference
} from "@markra/markdown";

export type WorkspaceResourceWorkerRequest = {
  documents: Array<{ content: string; path: string }>;
  scanId: number;
  type: "analyze";
};

export type WorkspaceResourceWorkerResponse =
  | {
      occurrences: Array<{
        path: string;
        references: MarkdownResourceReference[];
      }>;
      scanId: number;
      type: "analyzed";
    }
  | {
      error: string;
      path: string;
      scanId: number;
      type: "failed";
    };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validScanId(value: unknown) {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isWorkerDocument(value: unknown): value is WorkspaceResourceWorkerRequest["documents"][number] {
  if (!isRecord(value)) return false;

  return typeof value.content === "string" &&
    typeof value.path === "string" &&
    value.path.trim().length > 0;
}

function isReference(value: unknown): value is MarkdownResourceReference {
  if (!isRecord(value)) return false;

  return typeof value.columnNumber === "number" &&
    typeof value.from === "number" &&
    typeof value.href === "string" &&
    (value.kind === "image" || value.kind === "attachment") &&
    typeof value.lineNumber === "number" &&
    typeof value.text === "string" &&
    typeof value.to === "number";
}

export function isWorkspaceResourceWorkerRequest(
  value: unknown
): value is WorkspaceResourceWorkerRequest {
  if (!isRecord(value)) return false;

  return value.type === "analyze" &&
    validScanId(value.scanId) &&
    Array.isArray(value.documents) &&
    value.documents.length > 0 &&
    value.documents.length <= 4 &&
    value.documents.every(isWorkerDocument);
}

export function isWorkspaceResourceWorkerResponse(
  value: unknown
): value is WorkspaceResourceWorkerResponse {
  if (!isRecord(value) || !validScanId(value.scanId)) return false;

  if (value.type === "failed") {
    return typeof value.error === "string" && value.error.length > 0 &&
      typeof value.path === "string" && value.path.length > 0;
  }
  if (value.type !== "analyzed" || !Array.isArray(value.occurrences)) return false;

  return value.occurrences.every((occurrence) =>
    isRecord(occurrence) &&
    typeof occurrence.path === "string" &&
    occurrence.path.length > 0 &&
    Array.isArray(occurrence.references) &&
    occurrence.references.every(isReference)
  );
}

function errorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  return "Unable to parse Markdown resource references.";
}

export function analyzeWorkspaceResourceBatch(
  request: WorkspaceResourceWorkerRequest
): WorkspaceResourceWorkerResponse[] {
  const failures: WorkspaceResourceWorkerResponse[] = [];
  const occurrences: Extract<WorkspaceResourceWorkerResponse, { type: "analyzed" }>["occurrences"] = [];

  for (const value of request.documents as unknown[]) {
    if (!isWorkerDocument(value)) {
      const path = isRecord(value) && typeof value.path === "string" && value.path
        ? value.path
        : "<unknown>";
      failures.push({
        error: "Invalid Markdown worker document.",
        path,
        scanId: request.scanId,
        type: "failed"
      });
      continue;
    }

    try {
      occurrences.push({
        path: value.path,
        references: parseMarkdownResourceReferences(value.content)
      });
    } catch (error) {
      failures.push({
        error: errorMessage(error),
        path: value.path,
        scanId: request.scanId,
        type: "failed"
      });
    }
  }

  return [
    ...failures,
    { occurrences, scanId: request.scanId, type: "analyzed" }
  ];
}

export function createWorkspaceResourceWorker() {
  return new Worker(
    new URL("../workers/workspace-resource-scan.worker.ts", import.meta.url),
    { type: "module" }
  );
}
