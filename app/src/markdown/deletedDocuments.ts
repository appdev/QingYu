export interface DeletedMarkdownEntry {
    id: string;
    notebook: string;
    originalPath: string;
    historyPath: string;
    deletedAt: number;
    size: number;
    revision: string;
}

export interface DeletedMarkdownTarget {
    notebook: string;
    parentPath: string;
    name: string;
}

type MarkdownRequest = (
    url: string,
    body: Record<string, unknown>,
) => Promise<{code: number; data?: unknown; msg?: string}>;

const requestMarkdown: MarkdownRequest = async (url, body) => {
    const response = await fetch(url, {method: "POST", body: JSON.stringify(body)});
    return response.json();
};

const originalTarget = (entry: DeletedMarkdownEntry): DeletedMarkdownTarget => {
    const separator = entry.originalPath.lastIndexOf("/");
    return {
        notebook: entry.notebook,
        parentPath: separator <= 0 ? "/" : entry.originalPath.slice(0, separator),
        name: entry.originalPath.slice(separator + 1),
    };
};

export const loadDeletedMarkdown = async (request: MarkdownRequest = requestMarkdown) => {
    const response = await request("/api/markdown/listDeleted", {});
    if (response.code !== 0 || !Array.isArray(response.data)) return [];
    return [...response.data as DeletedMarkdownEntry[]].sort((left, right) => right.deletedAt - left.deletedAt);
};

export const previewDeletedMarkdown = async (id: string, request: MarkdownRequest = requestMarkdown) => {
    const response = await request("/api/markdown/getDeleted", {id});
    if (response.code !== 0 || !response.data || typeof response.data !== "object") return "";
    const data = response.data as Record<string, unknown>;
    return typeof data.content === "string" ? data.content : "";
};

export const setDeletedMarkdownPreview = (element: HTMLTextAreaElement, content: string) => {
    element.value = content;
};

const deletedMarkdownActionTypes = new Set([
    "restoreDeletedMarkdown",
    "restoreDeletedMarkdownTo",
    "purgeDeletedMarkdown",
]);

export const deletedMarkdownActionAllowed = (target: Element, readonly: boolean) =>
    !readonly || !deletedMarkdownActionTypes.has(target.getAttribute("data-type") || "");

export const renderDeletedMarkdownList = (element: HTMLElement, entries: readonly DeletedMarkdownEntry[], options: {
    readonly: boolean;
    emptyText: string;
    restoreText: string;
    restoreToText?: string;
    purgeText: string;
    formatTime?(time: number): string;
}) => {
    element.replaceChildren();
    if (entries.length === 0) {
        const empty = element.ownerDocument.createElement("li");
        empty.className = "b3-list--empty";
        empty.textContent = options.emptyText;
        element.append(empty);
        return;
    }
    entries.forEach((entry) => {
        const item = element.ownerDocument.createElement("li");
        item.className = "b3-list-item b3-list-item--hide-action";
        item.dataset.type = "deletedMarkdownItem";
        item.dataset.id = entry.id;
        const text = element.ownerDocument.createElement("span");
        text.className = "b3-list-item__text";
        text.title = entry.originalPath;
        text.textContent = entry.originalPath.slice(entry.originalPath.lastIndexOf("/") + 1);
        const meta = element.ownerDocument.createElement("span");
        meta.className = "b3-list-item__meta";
        meta.textContent = options.formatTime?.(entry.deletedAt) ?? String(entry.deletedAt);
        item.append(text, meta);
        if (!options.readonly) {
            [
                ["restoreDeletedMarkdown", options.restoreText, "iconUndo"],
                ["restoreDeletedMarkdownTo", options.restoreToText ?? options.restoreText, "iconMove"],
                ["purgeDeletedMarkdown", options.purgeText, "iconTrashcan"],
            ].forEach(([type, label, icon]) => {
                const action = element.ownerDocument.createElement("span");
                action.className = "b3-list-item__action ariaLabel";
                action.dataset.type = type;
                action.setAttribute("aria-label", label);
                action.innerHTML = `<svg><use xlink:href="#${icon}"></use></svg>`;
                item.append(action);
            });
        }
        element.append(item);
    });
};

export const resolveDeletedMarkdownTarget = (
    entry: DeletedMarkdownEntry,
    notebooks: ReadonlySet<string>,
) => notebooks.has(entry.notebook) ? originalTarget(entry) : null;

export const buildRestoreDeletedMarkdownRequest = (
    entry: DeletedMarkdownEntry,
    target?: DeletedMarkdownTarget,
) => {
    const resolved = target || originalTarget(entry);
    return {
        id: entry.id,
        toNotebook: resolved.notebook,
        toParentPath: resolved.parentPath,
        name: resolved.name,
    };
};

export const buildPurgeDeletedMarkdownRequest = (entry: DeletedMarkdownEntry) => ({id: entry.id});

export const restoreDeletedMarkdown = async (
    entry: DeletedMarkdownEntry,
    target?: DeletedMarkdownTarget,
    request: MarkdownRequest = requestMarkdown,
) => {
    const operationID = createMarkdownManagementOperationID();
    beginMarkdownManagementOperation(operationID);
    let response: Awaited<ReturnType<MarkdownRequest>>;
    try {
        response = await request("/api/markdown/restore", {...buildRestoreDeletedMarkdownRequest(entry, target), operationID});
    } catch (error) {
        cancelMarkdownManagementOperation(operationID);
        return {ok: false, conflict: false, message: error instanceof Error ? error.message : ""};
    }
    if (response.code === 0 && (response.data as Record<string, unknown> | undefined)?.operationID === operationID) {
        completeMarkdownManagementOperation(operationID);
        return {ok: true, conflict: false, message: ""};
    }
    cancelMarkdownManagementOperation(operationID);
    return {ok: false, conflict: response.code === 409, message: response.msg || ""};
};

export const purgeDeletedMarkdown = async (
    entry: DeletedMarkdownEntry,
    request: MarkdownRequest = requestMarkdown,
) => {
    const operationID = createMarkdownManagementOperationID();
    beginMarkdownManagementOperation(operationID);
    let response: Awaited<ReturnType<MarkdownRequest>>;
    try {
        response = await request("/api/markdown/purgeDeleted", {...buildPurgeDeletedMarkdownRequest(entry), operationID});
    } catch (error) {
        cancelMarkdownManagementOperation(operationID);
        return {ok: false, conflict: false, message: error instanceof Error ? error.message : ""};
    }
    if (response.code === 0 && (response.data as Record<string, unknown> | undefined)?.operationID === operationID) {
        completeMarkdownManagementOperation(operationID);
        return {ok: true, conflict: false, message: ""};
    }
    cancelMarkdownManagementOperation(operationID);
    return {ok: false, conflict: response.code === 409, message: response.msg || ""};
};
import {
    beginMarkdownManagementOperation,
    cancelMarkdownManagementOperation,
    completeMarkdownManagementOperation,
    createMarkdownManagementOperationID,
} from "./documentManagement";
