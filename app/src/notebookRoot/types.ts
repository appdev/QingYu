export type NotebookRootView = "large" | "masonry" | "list";

export interface NotebookRootDocument {
    kind: "sy" | "markdown";
    notebook: string;
    path: string;
    documentID: string;
    identityState: "valid" | "missing" | "invalid-value" | "malformed";
    identityConflict: boolean;
    revision: string;
    cardRatio: number;
    title: string;
    previewText: string;
    icon: string;
    created: number;
    updated: number;
    size: number;
    sort: number;
    subFileCount: number;
}

export interface NotebookRootListing {
    notebook: string;
    name: string;
    icon: string;
    sortMode: number;
    documents: NotebookRootDocument[];
}
