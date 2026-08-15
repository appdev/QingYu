import {EditorState, type Extension} from "@codemirror/state";
import {
    EditorView,
    highlightActiveLine,
    highlightActiveLineGutter,
    lineNumbers,
    placeholder,
} from "@codemirror/view";
import {getCodeLanguages} from "../protyle/codeLanguage";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {markdownHostAdapter} from "./markra-core/adapter";
import {readSiyuanCodeBlockConfig} from "./codeBlockConfig";
import {markdownSelectAllExtension} from "./markdownSelectAll";
import {
    blocksPlugin,
    type BlockLabels,
    calloutPreviewPlugin,
    codeBlockPreviewPlugin,
    codeMirrorBlockDragPlugin,
    codeMirrorClipboardAssetsPlugin,
    footnotePreviewPlugin,
    foldTogglePlugin,
    formattingPlugin,
    frontmatterHiddenPlugin,
    horizontalRulePlugin,
    imageAtomicEditingPlugin,
    imagePreviewPlugin,
    insertionsPlugin,
    linksPlugin,
    liveMarkdown,
    markdownEditingPlugin,
    markdownShortcutsPlugin,
    markdownSourceSyntaxHighlighting,
    markraLanguage,
    mathPreviewPlugin,
    rawHtmlPreviewPlugin,
    resolveSafeLinkTarget,
    tableFragmentMergePlugin,
    tablePreviewPlugin,
    trailingSpacePlugin,
} from "./markra-core/codemirror";

const getBlockLabels = (): Partial<BlockLabels> => {
    const languages = window.siyuan?.languages;
    const labels: Partial<BlockLabels> = {};
    const assign = (id: keyof BlockLabels, label?: string) => {
        if (label) {
            labels[id] = label;
        }
    };
    assign("block.paragraph", languages?.paragraph);
    assign("block.heading.1", languages?.heading1);
    assign("block.heading.2", languages?.heading2);
    assign("block.heading.3", languages?.heading3);
    assign("block.heading.4", languages?.heading4);
    assign("block.heading.5", languages?.heading5);
    assign("block.heading.6", languages?.heading6);
    assign("block.bullet-list", languages?.unorderedList);
    assign("block.quote", languages?.quote);
    assign("block.callout", languages?.callout);
    assign("block.table", languages?.table);
    return labels;
};

export interface SiyuanMarkraExtensionOptions {
    adapter: MarkdownHostAdapter;
    documentPath(): string;
    getScrollContainer?(view: EditorView): HTMLElement | null;
    mode: "source" | "visual";
}

const editorTheme = EditorView.theme({
    "&": {
        backgroundColor: "transparent",
        color: "var(--b3-theme-on-background)",
        height: "100%",
    },
    "&.cm-focused": {outline: "none"},
    ".cm-content": {
        minHeight: "100%",
        padding: "0",
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
    },
    ".cm-line": {padding: "0"},
    ".cm-scroller": {
        fontFamily: "inherit",
        lineHeight: "inherit",
        overflow: "visible",
    },
});

const sourceModeAppearance: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    EditorView.editorAttributes.of({"data-markdown-mode": "source"}),
];

const visualModeAppearance: Extension[] = [
    EditorView.editorAttributes.of({"data-markdown-mode": "visual"}),
];

export const createSiyuanModeAppearanceExtension = (mode: "source" | "visual"): Extension =>
    mode === "source" ? sourceModeAppearance : visualModeAppearance;

export const createSiyuanMarkraExtension = ({
    adapter,
    documentPath,
    getScrollContainer,
    mode,
}: SiyuanMarkraExtensionOptions): Extension => {
    const imageOptions = {
        className: "img",
        resolveSource: ({source}: {source: string}) => adapter.resolveImageSource(source, documentPath()),
    };
    const linkOptions = {
        open: ({target}: {target: string}) => adapter.openLink(target),
        resolveTarget: ({source}: {source: string}) => resolveSafeLinkTarget(source),
    };
    const common: Extension[] = [
        markdownHostAdapter(adapter),
        EditorView.lineWrapping,
        createSiyuanModeAppearanceExtension(mode),
        EditorView.contentAttributes.of({
            "aria-label": "Markdown",
            "aria-multiline": "true",
            "data-language": "markdown",
            role: "textbox",
            spellcheck: "false",
        }),
        placeholder(window.siyuan?.languages?.emptyPlaceholder ?? ""),
        editorTheme,
        markdownSelectAllExtension(),
    ];
    if (mode === "source") {
        return [...common, markraLanguage, markdownSourceSyntaxHighlighting];
    }
    const saveImage = async (file: File) => {
        const [saved] = await adapter.saveClipboardAssets({files: [file], insertionOffset: 0});
        return saved ? {alt: file.name, src: saved.markdownDestination} : null;
    };
    const saveAttachment = async (file: File) => {
        const [saved] = await adapter.saveClipboardAssets({files: [file], insertionOffset: 0});
        return saved ? {label: file.name, src: saved.markdownDestination} : null;
    };
    const codeBlockConfig = readSiyuanCodeBlockConfig(window.siyuan?.config?.editor);
    return [
        ...common,
        liveMarkdown({
            plugins: [
                blocksPlugin({
                    keybindings: false,
                    labels: getBlockLabels(),
                }),
                calloutPreviewPlugin(),
                codeBlockPreviewPlugin({
                    ...codeBlockConfig,
                    icons: {
                        check: "#iconCheck",
                        copy: "#iconCopy",
                        more: "#iconMore",
                    },
                    labels: {
                        clearLanguage: window.siyuan?.languages?.clear ?? "Clear",
                        searchLanguage: window.siyuan?.languages?.search ?? "Search",
                    },
                    languages: () => getCodeLanguages()
                        .map((language) => ({label: language, value: language})),
                    openCodeLanguageMenu: adapter.openCodeLanguageMenu,
                    positionLanguagePopover: adapter.positionPopover,
                    updateLanguages: (detail) => adapter.updateCodeLanguages?.(detail) ?? detail.languages,
                }),
                codeMirrorBlockDragPlugin({getScrollContainer}),
                codeMirrorClipboardAssetsPlugin({
                    convertHtmlToMarkdown: adapter.convertHtmlToMarkdown,
                    saveAttachment,
                    saveImage,
                }),
                footnotePreviewPlugin(),
                foldTogglePlugin(),
                formattingPlugin({keybindings: false}),
                frontmatterHiddenPlugin(window.siyuan?.languages?.emptyPlaceholder ?? ""),
                horizontalRulePlugin(),
                imageAtomicEditingPlugin(),
                imagePreviewPlugin(imageOptions),
                insertionsPlugin({
                    labels: window.siyuan?.languages?.date ? {"insert.today": window.siyuan.languages.date} : {},
                }),
                linksPlugin(linkOptions),
                mathPreviewPlugin(),
                markdownEditingPlugin(),
                markdownShortcutsPlugin(),
                rawHtmlPreviewPlugin({
                    resolveImageSrc: (source) => adapter.resolveImageSource(source, documentPath()) || source,
                }),
                tableFragmentMergePlugin(),
                tablePreviewPlugin({
                    getDocumentKey: documentPath,
                    images: imageOptions,
                    links: linkOptions,
                    widthMode: "auto",
                }),
                trailingSpacePlugin(),
            ],
            slashMenu: true,
        }),
        EditorState.readOnly.of(false),
    ];
};
