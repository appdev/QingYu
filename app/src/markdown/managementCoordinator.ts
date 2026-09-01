import {flushMarkdownDocumentEditors, MarkdownDocumentEditor, MarkdownDocumentReference} from "./documentManagement";

const REGISTER_CHANNEL = "siyuan-markdown-management-register";
const INVOKE_CHANNEL = "siyuan-markdown-management-invoke";
const PREPARE_CHANNEL = "siyuan-markdown-management-prepare";
const ACK_CHANNEL = "siyuan-markdown-management-ack";
const READY_CHANNEL = "siyuan-markdown-management-ready";

export interface MarkdownManagementPrepareRequest {
    phase: "prepare";
    generation: number;
    operationID: string;
    ref?: MarkdownDocumentReference;
    mode: "flush" | "presence" | "barrier";
    excludedEditorID?: string;
    expectedRevision?: string;
    workspace?: string;
}

export interface MarkdownManagementPrepareResult {
    operationID: string;
    ok: boolean;
    matched: boolean;
    matches: number;
    revision?: string;
    phase: "prepare";
    generation: number;
    mode: "flush" | "presence" | "barrier";
    workspace?: string;
}

export interface MarkdownManagementCommitRequest {
    phase: "commit";
    generation: number;
    operationID: string;
    workspace?: string;
    mutation: {
        kind: string;
        from?: MarkdownDocumentReference;
        to?: MarkdownDocumentReference;
        revision?: string;
        title?: string;
    };
}

let activePrepareDepth = 0;
export const isMarkdownManagementPrepareActive = () => activePrepareDepth > 0;

export const handleMarkdownManagementPrepare = async (
    request: MarkdownManagementPrepareRequest,
    editors: readonly MarkdownDocumentEditor[],
): Promise<MarkdownManagementPrepareResult> => {
    const envelope = {phase: "prepare" as const, generation: request.generation, operationID: request.operationID,
        mode: request.mode, ...(request.workspace ? {workspace: request.workspace} : {})};
    if (request.mode === "barrier") {
        return {...envelope, ok: true, matched: false, matches: 0};
    }
    if (!request.ref) {
        return {...envelope, ok: false, matched: false, matches: 0};
    }
    const matches = editors.filter((editor) => (!request.excludedEditorID || editor.managementID !== request.excludedEditorID) &&
        editor.notebookId === request.ref.notebook && editor.path === request.ref.path);
    if (request.mode === "presence") {
        return {...envelope, ok: true, matched: matches.length > 0, matches: matches.length};
    }
    if (matches.length === 0) {
        return {...envelope, ok: true, matched: false, matches: 0};
    }
    activePrepareDepth++;
    let revision: string | null;
    try {
        revision = await flushMarkdownDocumentEditors(request.ref, matches);
    } finally {
        activePrepareDepth--;
    }
    return revision
        ? {...envelope, ok: true, matched: true, matches: matches.length, revision}
        : {...envelope, ok: false, matched: true, matches: matches.length};
};

export const handleMarkdownManagementCommit = async (
    request: MarkdownManagementCommitRequest,
    editors: readonly MarkdownDocumentEditor[],
) => {
    const mutation = request.mutation;
    const matches = mutation.from ? editors.filter((editor) => editor.notebookId === mutation.from.notebook &&
        editor.path === mutation.from.path) : [];
    try {
        if (mutation.kind === "remove") {
            await Promise.all(matches.map((editor) => editor.close?.()));
        } else if (mutation.kind === "save" && mutation.revision) {
            matches.forEach((editor) => editor.applyWorkspaceDocumentRevision?.(mutation.revision));
        } else if (mutation.kind === "rename" && mutation.to && mutation.revision && mutation.title) {
            matches.forEach((editor) => editor.applyWorkspaceDocumentRename?.(
                mutation.to.notebook,
                mutation.to.path,
                mutation.revision,
                mutation.title,
            ));
        } else if (mutation.to && mutation.revision) {
            matches.forEach((editor) => editor.applyWorkspaceDocumentReference?.(
                mutation.to.notebook,
                mutation.to.path,
                mutation.revision,
            ));
        }
        return {phase: "commit" as const, generation: request.generation, operationID: request.operationID,
            ...(request.workspace ? {workspace: request.workspace} : {}), ok: true};
    } catch {
        return {phase: "commit" as const, generation: request.generation, operationID: request.operationID,
            ...(request.workspace ? {workspace: request.workspace} : {}), ok: false};
    }
};

export interface MarkdownManagementIPC {
    invoke(channel: string, payload: unknown): Promise<{ok: boolean; revision?: string; matches: number; lease?: string}>;
    send(channel: string, payload: unknown): void;
    on(channel: string, listener: (_event: unknown, payload: MarkdownManagementPrepareRequest | MarkdownManagementCommitRequest) => void): void;
}

export const markdownCoordinatorEditor = (editor: {
    managementID?: string;
    notebookId: string;
    path: string;
    flush(): Promise<boolean>;
}): MarkdownDocumentEditor => {
    const candidate = editor as typeof editor & {
        getRevision?(): string;
        isReadOnly?(): boolean;
        applyWorkspaceDocumentRevision?(revision: string): void;
        applyWorkspaceDocumentReference?(notebook: string, path: string, revision: string): void;
        applyWorkspaceDocumentRename?(notebook: string, path: string, revision: string, title: string): void;
        close?(): void | Promise<void>;
    };
    return ({
        managementID: editor.managementID,
        get notebookId() { return editor.notebookId; },
        get path() { return editor.path; },
        flush: () => editor.flush(),
        getRevision: () => candidate.getRevision?.() ?? "",
        isReadOnly: () => candidate.isReadOnly?.() ?? false,
        applyWorkspaceDocumentRevision: (revision) => candidate.applyWorkspaceDocumentRevision?.(revision),
        applyWorkspaceDocumentReference: (notebook, path, revision) =>
            candidate.applyWorkspaceDocumentReference?.(notebook, path, revision),
        applyWorkspaceDocumentRename: (notebook, path, revision, title) =>
            candidate.applyWorkspaceDocumentRename?.(notebook, path, revision, title),
        close: () => candidate.close?.(),
    });
};

export const installMarkdownManagementRendererCoordinator = (
    ipc: MarkdownManagementIPC,
    workspace: string,
    getEditors: () => readonly MarkdownDocumentEditor[],
) => {
    const register = () => ipc.send(REGISTER_CHANNEL, {workspace});
    register();
    ipc.on(READY_CHANNEL, register);
    ipc.on(PREPARE_CHANNEL, (_event, request) => {
        const handled = request.phase === "commit"
            ? handleMarkdownManagementCommit(request, getEditors())
            : handleMarkdownManagementPrepare(request, getEditors());
        void handled.then((result) => {
            ipc.send(ACK_CHANNEL, result);
        });
    });
};

export const prepareMarkdownMutationAcrossRenderers = async (
    ipc: MarkdownManagementIPC | undefined,
    workspace: string,
    ref: MarkdownDocumentReference,
    editors: readonly MarkdownDocumentEditor[],
    operationID: string,
    options: {expectedRevision?: string; excludedEditorID?: string; mode?: "flush" | "barrier"} = {},
) => {
    const mode = options.mode ?? "flush";
    if (!ipc) {
        if (mode === "barrier") return {ok: true, matches: 0, lease: operationID};
        const matches = editors.filter((editor) =>
            (!options.excludedEditorID || editor.managementID !== options.excludedEditorID) &&
            editor.notebookId === ref.notebook && editor.path === ref.path);
        if (matches.length === 0) return {ok: true, matches: 0, lease: operationID};
        const revision = await flushMarkdownDocumentEditors(ref, matches);
        return revision ? {ok: true, revision, matches: matches.length, lease: operationID} :
            {ok: false, matches: matches.length};
    }
    try {
        const result = await ipc.invoke(INVOKE_CHANNEL, {
            action: "prepare",
            workspace,
            operationID,
            ref,
            mode,
            expectedRevision: options.expectedRevision,
            excludedEditorID: options.excludedEditorID,
        });
        return result;
    } catch {
        return {ok: false, matches: 0};
    }
};

export const commitMarkdownMutationAcrossRenderers = async (
    ipc: MarkdownManagementIPC | undefined,
    workspace: string,
    operationID: string,
    lease: string,
    mutation: MarkdownManagementCommitRequest["mutation"],
    editors: readonly MarkdownDocumentEditor[],
) => {
    if (!ipc) {
        const result = await handleMarkdownManagementCommit({phase: "commit", generation: 0, operationID, mutation}, editors);
        return result.ok;
    }
    try {
        return (await ipc.invoke(INVOKE_CHANNEL, {action: "commit", workspace, operationID, lease, mutation})).ok;
    } catch {
        return false;
    }
};

export const abortMarkdownMutationAcrossRenderers = async (
    ipc: MarkdownManagementIPC | undefined,
    workspace: string,
    operationID: string,
    lease: string,
) => {
    if (!ipc) return;
    try {
        await ipc.invoke(INVOKE_CHANNEL, {action: "abort", workspace, operationID, lease});
    } catch {
        // Renderer 销毁时主进程会按 workspace generation 清理租约。
    }
};

export const countMarkdownPresenceAcrossRenderers = async (
    ipc: MarkdownManagementIPC | undefined,
    workspace: string,
    ref: MarkdownDocumentReference,
    editors: readonly MarkdownDocumentEditor[],
    operationID: string,
) => {
    if (!ipc) return editors.filter((editor) => editor.notebookId === ref.notebook && editor.path === ref.path).length;
    try {
        const result = await ipc.invoke(INVOKE_CHANNEL, {
            action: "prepare",
            workspace,
            operationID,
            ref,
            mode: "presence",
        });
        return result.ok ? result.matches : null;
    } catch {
        return null;
    }
};
