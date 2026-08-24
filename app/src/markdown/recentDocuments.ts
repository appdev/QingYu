export type RecentDocumentTimestamp = "viewedAt" | "openAt" | "closedAt" | "updated";

interface RecentDocumentBase {
    title: string;
    icon?: string;
    viewedAt?: number;
    openAt?: number;
    closedAt?: number;
    updated?: number;
}

export type RecentDocumentItem = RecentDocumentBase & ({
    kind: "native";
    rootID: string;
} | {
    kind: "markdown";
    notebook: string;
    path: string;
});

interface RecentDocumentOpeners<App> {
    openMarkdown(app: App, notebook: string, path: string, title: string): Promise<unknown> | unknown;
    openNative(app: App, rootID: string, title: string): Promise<unknown> | unknown;
}

interface ClosedMarkdownReference {
    kind: "markdown";
    notebook: string;
    path: string;
}

interface RecentlyClosedDependencies<App> {
    validateMarkdown(ref: ClosedMarkdownReference): Promise<boolean>;
    openMarkdown(app: App, ref: ClosedMarkdownReference, title: string, layout: unknown): Promise<void> | void;
    restoreNative(app: App, layout: unknown): Promise<boolean>;
    stale(ref: ClosedMarkdownReference): void;
    persist?(layouts: readonly unknown[]): void;
    limit?: number;
}

const escapeHTML = (value: string) => value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");

const closedChild = (value: unknown) => {
    if (!value || typeof value !== "object") return null;
    const record = value as Record<string, unknown>;
    return record.children && !Array.isArray(record.children) && typeof record.children === "object"
        ? record.children as Record<string, unknown>
        : record;
};

const closedMarkdownReference = (value: unknown): ClosedMarkdownReference | null => {
    const child = closedChild(value);
    if (child?.instance !== "MarkdownEditor" || typeof child.notebookId !== "string" || typeof child.path !== "string") return null;
    return {kind: "markdown", notebook: child.notebookId, path: child.path};
};

export const recentDocumentTimestamp = (item: RecentDocumentItem, field: RecentDocumentTimestamp) => item[field] || 0;

export const renderRecentDocumentItems = (items: readonly RecentDocumentItem[]) => items.map((item, index) => {
    const identity = item.kind === "markdown"
        ? `data-markdown-notebook="${escapeHTML(item.notebook)}" data-markdown-path="${escapeHTML(item.path)}"`
        : `data-node-id="${escapeHTML(item.rootID)}"`;
    return `<li data-index="${index}" ${identity} class="b3-list-item${index === 0 ? " b3-list-item--focus" : ""}"><span class="b3-list-item__text">${escapeHTML(item.title)}</span></li>`;
}).join("");

export const openRecentDocument = async <App>(
    app: App,
    item: RecentDocumentItem,
    openers: RecentDocumentOpeners<App>,
) => {
    if (item.kind === "markdown") {
        await openers.openMarkdown(app, item.notebook, item.path, item.title);
    } else {
        await openers.openNative(app, item.rootID, item.title);
    }
};

export const validateClosedMarkdownLayout = async (
    value: unknown,
    exists: (ref: ClosedMarkdownReference) => Promise<boolean>,
) => {
    const child = closedChild(value);
    if (child?.instance !== "MarkdownEditor") return true;
    const ref = closedMarkdownReference(value);
    return ref ? exists(ref) : false;
};

export const restoreRecentlyClosedTab = async <App>(
    app: App,
    closedTabs: unknown[],
    dependencies: RecentlyClosedDependencies<App>,
) => {
    const limit = dependencies.limit ?? closedTabs.length;
    for (let scanned = 0; scanned < limit && closedTabs.length > 0; scanned++) {
        const layout = closedTabs.at(-1);
        const child = closedChild(layout);
        if (child?.instance !== "MarkdownEditor" || typeof child.externalCapabilityId === "string") {
            closedTabs.pop();
            dependencies.persist?.(closedTabs);
            return dependencies.restoreNative(app, layout);
        }
        const ref = closedMarkdownReference(layout);
        if (!ref || !await dependencies.validateMarkdown(ref)) {
            closedTabs.pop();
            dependencies.persist?.(closedTabs);
            if (ref) dependencies.stale(ref);
            continue;
        }
        const title = typeof (layout as Record<string, unknown>).title === "string"
            ? (layout as Record<string, unknown>).title as string
            : ref.path.slice(ref.path.lastIndexOf("/") + 1);
        closedTabs.pop();
        dependencies.persist?.(closedTabs);
        await dependencies.openMarkdown(app, ref, title, layout);
        return true;
    }
    return false;
};
