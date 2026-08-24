export interface MarkdownDocumentReference {
    kind: "markdown";
    notebook: string;
    path: string;
}

export interface MarkdownDocumentEditor {
    managementID?: string;
    notebookId: string;
    path: string;
    readOnly?: boolean;
    revision?: string;
    flush(): Promise<boolean>;
    getRevision?(): string;
    isReadOnly?(): boolean;
    applyWorkspaceDocumentReference?(notebook: string, path: string, revision: string): void;
    close?(): void | Promise<void>;
}

export interface MarkdownDropTarget {
    notebook?: string;
    directory: string;
    kind?: "native" | "markdown";
}

export interface MarkdownFileTreeItem {
    kind: "native" | "markdown";
    path: string;
}

export interface MarkdownFileTreeDragItem extends MarkdownFileTreeItem {
    notebook: string;
}

export interface MarkdownFileTreeDropTarget {
    kind: "native" | "markdown";
    notebook: string;
    directory: string;
    mode: "child" | "sibling";
}

export interface MarkdownManagementEvent {
    cmd: string;
    kind: string;
    box: string;
    path: string;
    oldBox: string;
    oldPath: string;
    operationID: string;
    time: number;
}

interface NativeClosedDocumentReference {
    kind: "native";
    rootID: string;
}

export interface MarkdownManagementEventState {
    operationIDs: string[];
    openDocuments: MarkdownDocumentReference[];
    recentDocuments: MarkdownDocumentReference[];
    closedDocuments: Array<MarkdownDocumentReference | NativeClosedDocumentReference>;
}

export interface MarkdownManagementEventResult {
    applied: boolean;
    duplicate: boolean;
    event: MarkdownManagementEvent;
    refreshDirectories: string[];
    state: MarkdownManagementEventState;
}

export const markdownManagementEventFromWebSocket = (data: Pick<IWebSocketData, "cmd" | "data">) => ({
    ...(data.data && typeof data.data === "object" ? data.data : {}),
    cmd: data.cmd || "",
}) as MarkdownManagementEvent;

interface MarkdownManagementRuntimeEditor {
    notebookId: string;
    path: string;
    revision?: string;
    getRevision?(): string;
    applyWorkspaceDocumentReference?(notebook: string, path: string, revision: string): void;
    close?(): void | Promise<void>;
}

interface MarkdownManagementRuntime {
    editors: readonly MarkdownManagementRuntimeEditor[];
    closedTabs: unknown[];
    persistClosedTabs?(closedTabs: readonly unknown[]): void;
}

interface MarkdownFileTreeDropDependencies {
    sort(notebook: string, paths: string[]): Promise<boolean>;
    moveNative(paths: string[], notebook: string, directory: string): Promise<boolean>;
    moveMarkdown(ref: MarkdownDocumentReference, notebook: string, directory: string): Promise<boolean>;
    refresh?(notebook: string, directory: string): Promise<void>;
}

interface MarkdownMutationResponse {
    code: number;
    data?: Record<string, unknown> | null;
}

export interface MarkdownManagementMutation {
    kind: string;
    from?: MarkdownDocumentReference;
    to?: MarkdownDocumentReference;
    revision?: string;
}

export interface MarkdownManagementPreparedMutation {
    ok: boolean;
    lease?: string;
    revision?: string;
    matches?: number;
}

export const executeMarkdownManagementMutation = async (options: {
    operationID: string;
    prepare?(): Promise<MarkdownManagementPreparedMutation>;
    request(): Promise<MarkdownMutationResponse>;
    validate(response: MarkdownMutationResponse): boolean;
    mutation(response: MarkdownMutationResponse): MarkdownManagementMutation;
    commit?(lease: string, mutation: MarkdownManagementMutation): Promise<boolean>;
    abort?(lease: string): Promise<void> | void;
}) => {
    beginMarkdownManagementOperation(options.operationID);
    let prepared: MarkdownManagementPreparedMutation | undefined;
    const abort = async () => {
        if (!prepared?.lease || !options.abort) return;
        try {
            await options.abort(prepared.lease);
        } catch {
            // 协调器也会在 renderer 销毁或租约超时时清理，清理失败不应遮蔽原始请求错误。
        }
    };
    try {
        prepared = await options.prepare?.() ?? {ok: true, lease: options.operationID};
        if (!prepared.ok) {
            await abort();
            failMarkdownManagementOperation(options.operationID);
            return {ok: false as const};
        }
        const response = await options.request();
        if (!options.validate(response)) {
            await abort();
            failMarkdownManagementOperation(options.operationID);
            return {ok: false as const, response};
        }
        if (options.commit && (!prepared.lease || !await options.commit(prepared.lease, options.mutation(response)))) {
            await abort();
            failMarkdownManagementOperation(options.operationID);
            return {ok: false as const, response};
        }
        completeMarkdownManagementOperation(options.operationID);
        return {ok: true as const, response};
    } catch {
        await abort();
        failMarkdownManagementOperation(options.operationID);
        return {ok: false as const};
    }
};

const pendingMarkdownManagementOperations = new Set<string>();
const completedMarkdownManagementOperations = new Set<string>();
const pendingMarkdownManagementEvents = new Map<string, MarkdownManagementEvent>();
const markdownManagementReplayListeners = new Set<(event: MarkdownManagementEvent) => void>();

export const beginMarkdownManagementOperation = (operationID: string) => {
    pendingMarkdownManagementOperations.add(operationID);
};

export const completeMarkdownManagementOperation = (operationID: string) => {
    pendingMarkdownManagementOperations.delete(operationID);
    pendingMarkdownManagementEvents.delete(operationID);
    completedMarkdownManagementOperations.add(operationID);
    if (completedMarkdownManagementOperations.size > 256) {
        completedMarkdownManagementOperations.delete(completedMarkdownManagementOperations.values().next().value);
    }
};

export const failMarkdownManagementOperation = (operationID: string) => {
    pendingMarkdownManagementOperations.delete(operationID);
    const event = pendingMarkdownManagementEvents.get(operationID);
    pendingMarkdownManagementEvents.delete(operationID);
    if (event) markdownManagementReplayListeners.forEach((listener) => listener(event));
};

export const cancelMarkdownManagementOperation = failMarkdownManagementOperation;

export const subscribeMarkdownManagementOperationReplay = (listener: (event: MarkdownManagementEvent) => void) => {
    markdownManagementReplayListeners.add(listener);
    return () => markdownManagementReplayListeners.delete(listener);
};

export const isMarkdownManagementOperationHandled = (operationID: string) =>
    pendingMarkdownManagementOperations.has(operationID) || completedMarkdownManagementOperations.has(operationID);

interface MarkdownMutationDependencies {
    editors: readonly MarkdownDocumentEditor[];
    request(url: string, body: Record<string, unknown>): Promise<MarkdownMutationResponse>;
    loadRevision?(ref: MarkdownDocumentReference): Promise<string | null>;
    migrate?(from: MarkdownDocumentReference, to: MarkdownDocumentReference, revision: string): Promise<void> | void;
    close?(ref: MarkdownDocumentReference): Promise<void> | void;
    prepareRevision?(ref: MarkdownDocumentReference, operationID: string): Promise<{
        ok: boolean;
        revision?: string;
        matches: number;
        lease?: string;
    }>;
    commitMutation?(operationID: string, lease: string, mutation: MarkdownManagementMutation): Promise<boolean>;
    abortMutation?(operationID: string, lease: string): Promise<void> | void;
    createOperationID?(): string;
}

const parentDirectory = (path: string) => {
    const slash = path.lastIndexOf("/");
    return slash <= 0 ? "/" : path.slice(0, slash);
};

const markdownReferenceEquals = (left: MarkdownDocumentReference, right: MarkdownDocumentReference) =>
    left.notebook === right.notebook && left.path === right.path;

const migrateMarkdownReferences = (
    references: readonly MarkdownDocumentReference[],
    from: MarkdownDocumentReference,
    to: MarkdownDocumentReference,
) => references.map((reference) => markdownReferenceEquals(reference, from) ? to : reference);

const removeMarkdownReferences = (
    references: readonly MarkdownDocumentReference[],
    removed: MarkdownDocumentReference,
) => references.filter((reference) => !markdownReferenceEquals(reference, removed));

const supportedMarkdownManagementCommands = new Set([
    "createMarkdown",
    "saveMarkdown",
    "renameMarkdown",
    "removeMarkdown",
    "sortMarkdown",
    "purgeMarkdown",
]);

export const applyMarkdownManagementEvent = (
    state: MarkdownManagementEventState,
    event: MarkdownManagementEvent,
    options: {consumeInitiatingOperation?: boolean} = {},
): MarkdownManagementEventResult => {
    const ignored = !supportedMarkdownManagementCommands.has(event.cmd) || event.kind !== "markdown" ||
        !event.box || !event.path || !event.operationID || !Number.isFinite(event.time);
    if (ignored) {
        return {applied: false, duplicate: false, event, refreshDirectories: [], state};
    }
    if (pendingMarkdownManagementOperations.has(event.operationID)) {
        pendingMarkdownManagementEvents.set(event.operationID, event);
        if (!options.consumeInitiatingOperation) {
            return {applied: false, duplicate: true, event, refreshDirectories: [], state};
        }
    }
    if (state.operationIDs.includes(event.operationID) ||
        completedMarkdownManagementOperations.has(event.operationID) && !options.consumeInitiatingOperation) {
        return {applied: false, duplicate: true, event, refreshDirectories: [], state};
    }

    const next: MarkdownManagementEventState = {
        operationIDs: [...state.operationIDs, event.operationID].slice(-256),
        openDocuments: [...state.openDocuments],
        recentDocuments: [...state.recentDocuments],
        closedDocuments: [...state.closedDocuments],
    };
    const refreshDirectories = new Set<string>();
    const addRefresh = (notebook: string, path: string) => {
        if (notebook && path) refreshDirectories.add(`${notebook}:${parentDirectory(path)}`);
    };

    if (event.cmd === "renameMarkdown" && event.oldBox && event.oldPath) {
        const from: MarkdownDocumentReference = {kind: "markdown", notebook: event.oldBox, path: event.oldPath};
        const to: MarkdownDocumentReference = {kind: "markdown", notebook: event.box, path: event.path};
        next.openDocuments = migrateMarkdownReferences(next.openDocuments, from, to);
        next.recentDocuments = migrateMarkdownReferences(next.recentDocuments, from, to);
        next.closedDocuments = next.closedDocuments.map((reference) =>
            reference.kind === "markdown" && markdownReferenceEquals(reference, from) ? to : reference);
        addRefresh(event.oldBox, event.oldPath);
        addRefresh(event.box, event.path);
    } else if (event.cmd === "removeMarkdown") {
        const removed: MarkdownDocumentReference = {kind: "markdown", notebook: event.box, path: event.path};
        next.openDocuments = removeMarkdownReferences(next.openDocuments, removed);
        next.recentDocuments = removeMarkdownReferences(next.recentDocuments, removed);
        next.closedDocuments = next.closedDocuments.filter((reference) =>
            reference.kind !== "markdown" || !markdownReferenceEquals(reference, removed));
        addRefresh(event.box, event.path);
    } else if (event.cmd !== "purgeMarkdown") {
        addRefresh(event.box, event.path);
    }

    return {
        applied: true,
        duplicate: false,
        event,
        refreshDirectories: [...refreshDirectories],
        state: next,
    };
};

const closedLayoutChild = (layout: unknown) => {
    if (!layout || typeof layout !== "object") return null;
    const record = layout as Record<string, unknown>;
    return record.children && !Array.isArray(record.children) && typeof record.children === "object"
        ? record.children as Record<string, unknown>
        : record;
};

export const markdownReferenceFromLayout = (layout: unknown): MarkdownDocumentReference | null => {
    const child = closedLayoutChild(layout);
    if (child?.instance !== "MarkdownEditor" || typeof child.externalCapabilityId === "string" ||
        typeof child.notebookId !== "string" || typeof child.path !== "string") return null;
    return {kind: "markdown", notebook: child.notebookId, path: child.path};
};

export const markdownReferenceFromInitData = (initData: string | null | undefined) => {
    if (!initData) return null;
    try {
        return markdownReferenceFromLayout(JSON.parse(initData));
    } catch {
        return null;
    }
};

const closedLayoutMatches = (layout: unknown, reference: MarkdownDocumentReference) => {
    const child = markdownReferenceFromLayout(layout);
    return child?.notebook === reference.notebook && child.path === reference.path;
};

export const createMarkdownManagementEventState = (): MarkdownManagementEventState => ({
    operationIDs: [],
    openDocuments: [],
    recentDocuments: [],
    closedDocuments: [],
});

export const applyMarkdownManagementEventToRuntime = (
    state: MarkdownManagementEventState,
    event: MarkdownManagementEvent,
    runtime: MarkdownManagementRuntime,
): MarkdownManagementEventResult & {settled: Promise<void>} => {
    const result = applyMarkdownManagementEvent(state, event);
    if (!result.applied) return {...result, settled: Promise.resolve()};

    let settled = Promise.resolve();

    if (event.cmd === "renameMarkdown" && event.oldBox && event.oldPath) {
        const from: MarkdownDocumentReference = {kind: "markdown", notebook: event.oldBox, path: event.oldPath};
        runtime.editors.filter((editor) => editor.notebookId === from.notebook && editor.path === from.path)
            .forEach((editor) => editor.applyWorkspaceDocumentReference?.(
                event.box,
                event.path,
                editor.getRevision?.() ?? editor.revision ?? "",
            ));
        runtime.closedTabs.forEach((layout) => {
            if (!closedLayoutMatches(layout, from)) return;
            const child = closedLayoutChild(layout);
            child.notebookId = event.box;
            child.path = event.path;
        });
        runtime.persistClosedTabs?.(runtime.closedTabs);
    } else if (event.cmd === "removeMarkdown") {
        const removed: MarkdownDocumentReference = {kind: "markdown", notebook: event.box, path: event.path};
        const closeResults = runtime.editors
            .filter((editor) => editor.notebookId === removed.notebook && editor.path === removed.path)
            .map((editor) => editor.close?.());
        const removeClosedLayouts = () => {
            const remaining = runtime.closedTabs.filter((layout) => !closedLayoutMatches(layout, removed));
            runtime.closedTabs.splice(0, runtime.closedTabs.length, ...remaining);
            runtime.persistClosedTabs?.(runtime.closedTabs);
        };
        const asynchronousCloses = closeResults.filter((value): value is Promise<void> =>
            Boolean(value && typeof (value as Promise<void>).then === "function"));
        if (asynchronousCloses.length > 0) {
            settled = Promise.all(asynchronousCloses).then(removeClosedLayouts);
        } else {
            removeClosedLayouts();
        }
    }
    return {...result, settled};
};

export const createMarkdownManagementRuntimeEventController = (
    runtime: () => MarkdownManagementRuntime,
    initialState: MarkdownManagementEventState = createMarkdownManagementEventState(),
) => {
    let currentState = initialState;
    let currentSettlement = Promise.resolve();
    const handle = (event: MarkdownManagementEvent) => {
        const result = applyMarkdownManagementEventToRuntime(currentState, event, runtime());
        currentState = result.state;
        currentSettlement = result.settled;
        return result;
    };
    const unsubscribe = subscribeMarkdownManagementOperationReplay(handle);
    return {
        handle,
        state: () => currentState,
        settled: () => currentSettlement,
        destroy: unsubscribe,
    };
};

const matchingEditors = (ref: MarkdownDocumentReference, editors: readonly MarkdownDocumentEditor[]) => editors.filter((editor) =>
    editor.notebookId === ref.notebook && editor.path === ref.path);

const editorRevision = (editor: MarkdownDocumentEditor) => editor.getRevision?.() ?? editor.revision ?? "";

const editorReadOnly = (editor: MarkdownDocumentEditor) => editor.isReadOnly?.() ?? editor.readOnly ?? false;

export const flushMarkdownDocumentEditors = async (
    ref: MarkdownDocumentReference,
    editors: readonly MarkdownDocumentEditor[],
): Promise<string | null> => {
    const matches = matchingEditors(ref, editors);
    if (matches.length === 0 || matches.some(editorReadOnly)) return null;
    try {
        const flushed = await Promise.all(matches.map((editor) => editor.flush()));
        if (flushed.some((success) => !success)) return null;
    } catch {
        return null;
    }
    const revisions = new Set(matches.map(editorRevision));
    if (revisions.size !== 1) return null;
    const [revision] = revisions;
    return revision || null;
};

export const classifyMarkdownDrop = (
    source: Pick<MarkdownDocumentReference, "notebook" | "path">,
    target: MarkdownDropTarget,
): "sort" | "move" | "reject" => {
    if (target.kind === "markdown") return "reject";
    return (target.notebook === undefined || target.notebook === source.notebook) &&
        parentDirectory(source.path) === target.directory ? "sort" : "move";
};

export const orderedFileTreePaths = (items: readonly MarkdownFileTreeItem[]) => items.map((item) => item.path);

export const markdownFileTreeDragAttributes = () => 'data-doc-type="markdown" draggable="true"';

export const routeMarkdownFileTreeDrop = async (drop: {
    sources: readonly MarkdownFileTreeDragItem[];
    target: MarkdownFileTreeDropTarget;
    orderedPaths: string[];
}, dependencies: MarkdownFileTreeDropDependencies) => {
    const directories = new Map<string, [string, string]>();
    drop.sources.forEach((source) => {
        const directory = parentDirectory(source.path);
        directories.set(`${source.notebook}\u0000${directory}`, [source.notebook, directory]);
    });
    directories.set(`${drop.target.notebook}\u0000${drop.target.directory}`, [drop.target.notebook, drop.target.directory]);
    const refresh = async () => {
        for (const [notebook, directory] of directories.values()) {
            try {
                await dependencies.refresh?.(notebook, directory);
            } catch {
                // 刷新失败不应遮蔽原始拖拽结果，后续服务端事件仍会再次刷新。
            }
        }
    };
    if (drop.sources.length === 0 || drop.target.mode === "child" && drop.target.kind === "markdown") {
        await refresh();
        return {ok: false, reason: "invalid-target" as const};
    }
    const sameDirectory = drop.target.mode === "sibling" && drop.sources.every((source) =>
        source.notebook === drop.target.notebook && parentDirectory(source.path) === drop.target.directory);
    if (sameDirectory) {
        let sorted = false;
        try {
            sorted = await dependencies.sort(drop.target.notebook, drop.orderedPaths);
        } catch {
            sorted = false;
        }
        await refresh();
        return sorted ? {ok: true as const} : {ok: false as const, reason: "failed" as const};
    }

    const nativePaths = drop.sources.filter((source) => source.kind === "native").map((source) => source.path);
    const markdownSources = drop.sources.filter((source) => source.kind === "markdown");
    if (markdownSources.length > 1 || markdownSources.length > 0 && nativePaths.length > 0) {
        await refresh();
        return {ok: false, reason: "unsafe-mixed" as const};
    }
    let moved = true;
    try {
        if (nativePaths.length > 0) {
            moved = await dependencies.moveNative(nativePaths, drop.target.notebook, drop.target.directory);
        } else if (markdownSources.length === 1) {
            const source = markdownSources[0];
            moved = await dependencies.moveMarkdown({
                kind: "markdown",
                notebook: source.notebook,
                path: source.path,
            }, drop.target.notebook, drop.target.directory);
        }
    } catch {
        moved = false;
    }
    await refresh();
    return moved ? {ok: true as const} : {ok: false as const, reason: "failed" as const};
};

export const createMarkdownManagementOperationID = () => globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

export const createMarkdownDocument = async (input: {
    notebook: string;
    parentPath: string;
    name: string;
    autoName?: boolean;
}, dependencies: {
    request(url: string, body: Record<string, unknown>): Promise<MarkdownMutationResponse>;
    prepareOperation?(operationID: string): Promise<MarkdownManagementPreparedMutation>;
    commitMutation?(operationID: string, lease: string, mutation: MarkdownManagementMutation): Promise<boolean>;
    abortMutation?(operationID: string, lease: string): Promise<void> | void;
    createOperationID?(): string;
}) => {
    const operationID = dependencies.createOperationID?.() ?? createMarkdownManagementOperationID();
    const executed = await executeMarkdownManagementMutation({
        operationID,
        prepare: dependencies.prepareOperation ? () => dependencies.prepareOperation(operationID) : undefined,
        request: () => dependencies.request("/api/markdown/create", {...input, operationID}),
        validate: (response) => response.code === 0 && response.data?.operationID === operationID &&
            typeof response.data.path === "string" && typeof response.data.name === "string",
        mutation: () => ({kind: "create"}),
        commit: dependencies.commitMutation
            ? (lease, mutation) => dependencies.commitMutation(operationID, lease, mutation)
            : undefined,
        abort: dependencies.abortMutation ? (lease) => dependencies.abortMutation(operationID, lease) : undefined,
    });
    return executed.ok ? executed.response.data as Record<string, unknown> : null;
};

const mutationRevision = async (
    ref: MarkdownDocumentReference,
    dependencies: MarkdownMutationDependencies,
    operationID: string,
) => {
    if (dependencies.prepareRevision) {
        const prepared = await dependencies.prepareRevision(ref, operationID);
        if (!prepared.ok) return null;
        if (prepared.revision) return {revision: prepared.revision, lease: prepared.lease ?? operationID};
        if (prepared.matches > 0) return null;
        const revision = await dependencies.loadRevision?.(ref) ?? null;
        return revision ? {revision, lease: prepared.lease ?? operationID} : null;
    }
    const matches = matchingEditors(ref, dependencies.editors);
    const revision = await (matches.length > 0
        ? flushMarkdownDocumentEditors(ref, matches)
        : dependencies.loadRevision?.(ref) ?? null);
    return revision ? {revision, lease: operationID} : null;
};

const abortPreparedMarkdownMutation = async (
    dependencies: MarkdownMutationDependencies,
    operationID: string,
    lease: string,
) => {
    try {
        await dependencies.abortMutation?.(operationID, lease);
    } catch {
        // 主进程也会在 renderer 销毁或租约超时时清理，清理失败不应遮蔽原始请求错误。
    }
};

export const moveMarkdownDocument = async (
    ref: MarkdownDocumentReference,
    target: Pick<MarkdownDropTarget, "notebook" | "directory">,
    dependencies: MarkdownMutationDependencies,
) => {
    const operationID = dependencies.createOperationID?.() ?? createMarkdownManagementOperationID();
    const prepared = await mutationRevision(ref, dependencies, operationID);
    if (!prepared) return false;
    const {revision, lease} = prepared;
    if (!target.notebook) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        return false;
    }
    beginMarkdownManagementOperation(operationID);
    let response: MarkdownMutationResponse;
    try {
        response = await dependencies.request("/api/markdown/move", {
            notebook: ref.notebook, path: ref.path, revision, toNotebook: target.notebook,
            toParentPath: target.directory, operationID,
        });
    } catch {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    if (response.code !== 0 || !response.data || response.data.operationID !== operationID ||
        typeof response.data.path !== "string") {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    const next: MarkdownDocumentReference = {
        kind: "markdown",
        notebook: typeof response.data.notebook === "string" ? response.data.notebook : target.notebook,
        path: response.data.path,
    };
    const nextRevision = typeof response.data.revision === "string" ? response.data.revision : revision;
    if (dependencies.commitMutation && !await dependencies.commitMutation(operationID, lease,
        {kind: "move", from: ref, to: next, revision: nextRevision})) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    completeMarkdownManagementOperation(operationID);
    if (!dependencies.commitMutation) await dependencies.migrate?.(ref, next, nextRevision);
    return true;
};

export const recycleMarkdownDocument = async (
    ref: MarkdownDocumentReference,
    dependencies: MarkdownMutationDependencies,
) => {
    const operationID = dependencies.createOperationID?.() ?? createMarkdownManagementOperationID();
    const prepared = await mutationRevision(ref, dependencies, operationID);
    if (!prepared) return false;
    const {revision, lease} = prepared;
    beginMarkdownManagementOperation(operationID);
    let response: MarkdownMutationResponse;
    try {
        response = await dependencies.request("/api/markdown/remove", {
            notebook: ref.notebook, path: ref.path, revision, operationID,
        });
    } catch {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    if (response.code !== 0 || response.data?.operationID !== operationID) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    if (dependencies.commitMutation && !await dependencies.commitMutation(operationID, lease,
        {kind: "remove", from: ref})) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    completeMarkdownManagementOperation(operationID);
    if (!dependencies.commitMutation) await dependencies.close?.(ref);
    return true;
};

export const duplicateMarkdownDocument = async (
    ref: MarkdownDocumentReference,
    dependencies: MarkdownMutationDependencies,
) => {
    const operationID = dependencies.createOperationID?.() ?? createMarkdownManagementOperationID();
    const prepared = await mutationRevision(ref, dependencies, operationID);
    if (!prepared) return false;
    const {revision, lease} = prepared;
    beginMarkdownManagementOperation(operationID);
    let response: MarkdownMutationResponse;
    try {
        response = await dependencies.request("/api/markdown/duplicate", {
            notebook: ref.notebook, path: ref.path, revision, operationID,
        });
    } catch {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    if (response.code !== 0 || response.data?.operationID !== operationID) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    if (dependencies.commitMutation && !await dependencies.commitMutation(operationID, lease, {kind: "duplicate"})) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    completeMarkdownManagementOperation(operationID);
    return true;
};

export const renameMarkdownDocument = async (
    ref: MarkdownDocumentReference,
    name: string,
    dependencies: MarkdownMutationDependencies,
) => {
    const operationID = dependencies.createOperationID?.() ?? createMarkdownManagementOperationID();
    const prepared = await mutationRevision(ref, dependencies, operationID);
    if (!prepared) return false;
    const {revision, lease} = prepared;
    beginMarkdownManagementOperation(operationID);
    let response: MarkdownMutationResponse;
    try {
        response = await dependencies.request("/api/markdown/rename", {
            notebook: ref.notebook, path: ref.path, name, revision, operationID,
        });
    } catch {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    if (response.code !== 0 || !response.data || response.data.operationID !== operationID ||
        typeof response.data.path !== "string") {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        cancelMarkdownManagementOperation(operationID);
        return false;
    }
    const next: MarkdownDocumentReference = {
        kind: "markdown",
        notebook: ref.notebook,
        path: response.data.path,
    };
    const nextRevision = typeof response.data.revision === "string" ? response.data.revision : revision;
    if (dependencies.commitMutation && !await dependencies.commitMutation(operationID, lease,
        {kind: "rename", from: ref, to: next, revision: nextRevision})) {
        await abortPreparedMarkdownMutation(dependencies, operationID, lease);
        failMarkdownManagementOperation(operationID);
        return false;
    }
    completeMarkdownManagementOperation(operationID);
    if (!dependencies.commitMutation) await dependencies.migrate?.(ref, next, nextRevision);
    return true;
};
