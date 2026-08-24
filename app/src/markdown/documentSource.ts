export interface MarkdownDocument {
    name: string;
    displayPath: string;
    content: string;
    revision: string;
    mtime: number;
    utf8Bom: boolean;
    lineEnding: "\n" | "\r\n" | "\r";
    resourceToken?: string;
}

export interface MarkdownSaveRequest {
    content: string;
    revision: string;
    overwriteRevision?: string;
}

export interface MarkdownRenameRequest {
    name: string;
    revision: string;
}

export interface MarkdownMoveRequest {
    toNotebook: string;
    toParentPath: string;
    revision: string;
}

export type MarkdownMutationResult =
    | {status: "ok"; document: MarkdownDocument}
    | {status: "conflict"; revision: string}
    | {status: "error"; code: string};

export interface MarkdownSavedAsset {
    name: string;
    markdownDestination: string;
}

export interface MarkdownDocumentSource {
    readonly kind: "workspace" | "external";
    readonly key: string;
    readonly readOnly: boolean;
    load(): Promise<MarkdownDocument>;
    save(request: MarkdownSaveRequest): Promise<MarkdownMutationResult>;
    rename(request: MarkdownRenameRequest): Promise<MarkdownMutationResult>;
    duplicate?(revision: string): Promise<MarkdownMutationResult>;
    move?(request: MarkdownMoveRequest): Promise<MarkdownMutationResult>;
    resolveImageSource(source: string): string | null;
    openLink(target: string): Promise<void>;
    saveAssets(files: readonly File[]): Promise<readonly MarkdownSavedAsset[]>;
}

interface WorkspaceSourceOptions {
    notebookId: string;
    path: string;
    readOnly: boolean | (() => boolean);
    request(url: string, body: Record<string, unknown>): Promise<{code: number; data?: Record<string, unknown>}>;
    openLink?: (target: string) => Promise<void> | void;
    resolveImageSource?: (source: string) => string | null;
    saveAssets?: (files: readonly File[]) => Promise<readonly MarkdownSavedAsset[]>;
    createOperationID?(): string;
    prepareMutation?(
        ref: MarkdownDocumentReference,
        operationID: string,
        revision: string,
    ): Promise<MarkdownManagementPreparedMutation>;
    commitMutation?(
        operationID: string,
        lease: string,
        mutation: MarkdownManagementMutation,
    ): Promise<boolean>;
    abortMutation?(operationID: string, lease: string): Promise<void> | void;
    isPrepareActive?(): boolean;
}

const workspaceDocument = (value: Record<string, unknown>): MarkdownDocument => ({
    name: value.name as string,
    displayPath: value.path as string,
    content: value.content as string,
    revision: value.revision as string,
    mtime: value.mtime as number,
    utf8Bom: false,
    lineEnding: "\n",
});

export const createWorkspaceMarkdownDocumentSource = (options: WorkspaceSourceOptions): MarkdownDocumentSource => {
    let notebookId = options.notebookId;
    let documentPath = options.path;
    const isReadOnly = () => typeof options.readOnly === "function" ? options.readOnly() : options.readOnly;
    const mutate = async (
        url: string,
        body: Record<string, unknown>,
        kind: string,
        revision: string,
        destination?: {notebook: string; path?: string},
    ): Promise<MarkdownMutationResult> => {
        if (isReadOnly()) return {status: "error", code: "READ_ONLY"};
        const operationID = options.createOperationID?.() ?? createMarkdownManagementOperationID();
        const from: MarkdownDocumentReference = {kind: "markdown", notebook: notebookId, path: documentPath};
        const nestedPrepare = options.isPrepareActive?.() ?? false;
        const executed = await executeMarkdownManagementMutation({
            operationID,
            prepare: options.prepareMutation && !nestedPrepare
                ? () => options.prepareMutation(from, operationID, revision)
                : undefined,
            request: () => options.request(url, {...body, operationID}),
            validate: (response) => response.code === 0 && Boolean(response.data) &&
                response.data?.operationID === operationID && typeof response.data.path === "string",
            mutation: (response) => {
                const data = response.data as Record<string, unknown>;
                const to = kind === "duplicate" ? undefined : {
                    kind: "markdown" as const,
                    notebook: destination?.notebook ?? notebookId,
                    path: data.path as string,
                };
                return {kind, from: kind === "duplicate" ? undefined : from, to,
                    revision: typeof data.revision === "string" ? data.revision : revision};
            },
            commit: options.commitMutation && !nestedPrepare
                ? (lease, mutation) => options.commitMutation(operationID, lease, mutation)
                : undefined,
            abort: options.abortMutation && !nestedPrepare
                ? (lease) => options.abortMutation(operationID, lease)
                : undefined,
        });
        const response = executed.response;
        if (!executed.ok) {
            if (response?.code === 409) return {status: "conflict", revision: response.data?.revision as string || ""};
            const code = response?.code === 0 && response.data?.operationID !== operationID
                ? "OPERATION_MISMATCH"
                : `HTTP_${response?.code ?? "FAILED"}`;
            return {status: "error", code};
        }
        if (!response?.data) return {status: "error", code: "HTTP_FAILED"};
        if (typeof response.data.path === "string") documentPath = response.data.path;
        return {status: "ok", document: workspaceDocument(response.data)};
    };
    return {
        kind: "workspace",
        get key() { return `workspace:${notebookId}:${documentPath}`; },
        get readOnly() { return isReadOnly(); },
        async load() {
            const response = await options.request("/api/markdown/get", {notebook: notebookId, path: documentPath});
            if (response.code !== 0 || !response.data) throw new Error(`HTTP_${response.code}`);
            return workspaceDocument(response.data);
        },
        save(request) {
            return mutate("/api/markdown/save", {notebook: notebookId, path: documentPath, ...request},
                "save", request.revision);
        },
        rename(request) {
            return mutate("/api/markdown/rename", {notebook: notebookId, path: documentPath, ...request},
                "rename", request.revision);
        },
        duplicate(revision) {
            const currentPath = documentPath;
            return mutate("/api/markdown/duplicate", {notebook: notebookId, path: documentPath, revision},
                "duplicate", revision).then((result) => {
                documentPath = currentPath;
                return result;
            });
        },
        async move(request) {
            const result = await mutate("/api/markdown/move", {notebook: notebookId, path: documentPath, ...request},
                "move", request.revision, {notebook: request.toNotebook});
            if (result.status === "ok") notebookId = request.toNotebook;
            return result;
        },
        resolveImageSource(source) {
            return options.resolveImageSource?.(source) ?? source;
        },
        async openLink(target) {
            await options.openLink?.(target);
        },
        saveAssets(files) {
            return options.saveAssets ? options.saveAssets(files) : Promise.resolve([]);
        },
    };
};
import {
    createMarkdownManagementOperationID,
    executeMarkdownManagementMutation,
    MarkdownDocumentReference,
    MarkdownManagementMutation,
    MarkdownManagementPreparedMutation,
} from "./documentManagement";
