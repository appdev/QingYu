import {Facet, type EditorState} from "@codemirror/state";

export type MarkdownIconName = "add" | "dot" | "remove" | "trash" | "zoomIn" | "zoomOut" | "open";

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

export interface MarkdownCodeLanguageUpdate {
    languages: string[];
    listElement: HTMLElement;
    type: "init" | "match";
    value: string;
}

export interface MarkdownControlHandle {
    readonly element: HTMLElement;
    destroy(): void;
    focus(): void;
}

export interface MarkdownPopoverRequest {
    anchor: HTMLElement;
    content: HTMLElement;
    kind: "footnote" | "media" | "search";
    ownerDocument: Document;
    restoreFocus: boolean;
}

export interface MarkdownCodeLanguageMenuRequest {
    anchor: HTMLElement;
    currentLanguage: string;
    languages: readonly string[];
    onDestroy?(): void;
    onSelect(language: string): void;
    ownerDocument: Document;
}

export interface MarkdownHostAdapter {
    convertHtmlToMarkdown?(html: string): string | null | undefined;
    createIcon(name: MarkdownIconName, className: string, ownerDocument: Document): SVGElement;
    notifyError(message: string): void;
    mountPopover?(request: MarkdownPopoverRequest): MarkdownControlHandle | null;
    openCodeLanguageMenu?(request: MarkdownCodeLanguageMenuRequest): MarkdownControlHandle | null;
    openLink(target: string): void;
    positionPopover(anchor: HTMLElement, popover: HTMLElement): void;
    renderMath(source: string, displayMode: boolean, context: MarkdownRenderContext): HTMLElement;
    renderMermaid(source: string, context: MarkdownRenderContext): Promise<HTMLElement>;
    resolveImageSource(source: string, documentPath: string): string | null;
    saveClipboardAssets(request: MarkdownClipboardAssetRequest): Promise<readonly MarkdownSavedAsset[]>;
    updateCodeLanguages?(detail: MarkdownCodeLanguageUpdate): readonly string[];
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

export const readOptionalMarkdownHostAdapter = (state: EditorState) => state.facet(markdownHostAdapterFacet);

export const readMarkdownHostAdapter = (state: EditorState) => {
    const adapter = readOptionalMarkdownHostAdapter(state);
    if (!adapter) {
        throw new Error("Markdown editor host adapter is not configured");
    }
    return adapter;
};
