import {Facet, type EditorState} from "@codemirror/state";

export type MarkdownIconName = "add" | "remove" | "trash" | "zoomIn" | "zoomOut" | "open";

export interface MarkdownSavedAsset {
    markdownDestination: string;
    name: string;
}

export interface MarkdownClipboardAssetRequest {
    files: readonly File[];
    insertionOffset: number;
}

export interface MarkdownRenderContext {
    documentPath: string;
    ownerDocument: Document;
}

export interface MarkdownHostAdapter {
    createIcon(name: MarkdownIconName, className: string, ownerDocument: Document): SVGElement;
    notifyError(message: string): void;
    openLink(target: string): void;
    positionPopover(anchor: HTMLElement, popover: HTMLElement): void;
    renderMath(source: string, displayMode: boolean, context: MarkdownRenderContext): HTMLElement;
    renderMermaid(source: string, context: MarkdownRenderContext): Promise<HTMLElement>;
    resolveImageSource(source: string, documentPath: string): string | null;
    saveClipboardAssets(request: MarkdownClipboardAssetRequest): Promise<readonly MarkdownSavedAsset[]>;
}

export const markdownHostAdapterFacet = Facet.define<MarkdownHostAdapter, MarkdownHostAdapter | null>({
    combine(values) {
        if (values.length > 1) {
            throw new Error(`Markdown editor accepts one host adapter, received ${values.length}`);
        }
        return values[0] ?? null;
    },
});

export const markdownHostAdapter = (adapter: MarkdownHostAdapter) => markdownHostAdapterFacet.of(adapter);

export const readMarkdownHostAdapter = (state: EditorState) => {
    const adapter = state.facet(markdownHostAdapterFacet);
    if (!adapter) {
        throw new Error("Markdown editor host adapter is not configured");
    }
    return adapter;
};
