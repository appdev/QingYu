export interface NotebookRootDocumentIdentity {
    kind: "sy" | "markdown";
    notebook: string;
    id: string;
    path: string;
}

const normalizeNotebookRootPath = (path: string) => `/${path}`.replace(/\/{2,}/g, "/");

export const notebookRootDocumentKey = (identity: NotebookRootDocumentIdentity) => [
    identity.kind,
    identity.notebook,
    identity.id.trim() || normalizeNotebookRootPath(identity.path.trim()),
].join("\u001f");

export const notebookRootElementKey = (element: HTMLElement) => notebookRootDocumentKey({
    kind: element.dataset.kind as "sy" | "markdown",
    notebook: element.dataset.notebook || "",
    id: element.dataset.id || "",
    path: element.dataset.path || "",
});
