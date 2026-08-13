import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {history, undoDepth} from "@codemirror/commands";
import {EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import type {MarkdownHostAdapter} from "../markra-core/adapter";
import {createSiyuanMarkraExtension} from "../markraExtension";
import type {MarkdownAppearanceContract} from "./contracts";

export interface EditorContinuitySnapshot {
    view: EditorView;
    document: string;
    selection: {anchor: number; head: number};
    scrollTop: number;
    undoDepth: number;
}

export const captureEditorContinuity = (view: EditorView): EditorContinuitySnapshot => ({
    view,
    document: view.state.doc.toString(),
    selection: {
        anchor: view.state.selection.main.anchor,
        head: view.state.selection.main.head,
    },
    scrollTop: view.scrollDOM.scrollTop,
    undoDepth: undoDepth(view.state),
});

export const applyThemeCss = (css: string) => {
    const element = document.createElement("style");
    element.dataset.appearanceTestTheme = "true";
    element.textContent = css;
    document.head.append(element);
    return () => element.remove();
};

export const createTestHostAdapter = (): MarkdownHostAdapter => ({
    createIcon(_name, className, ownerDocument) {
        const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.setAttribute("class", className);
        return icon;
    },
    notifyError() {
        // 测试适配器不产生宿主副作用。
    },
    openLink() {
        // 测试适配器不打开外部链接。
    },
    positionPopover() {
        // JSDOM 测试不需要计算屏幕坐标。
    },
    renderMath(source, _displayMode, context) {
        const element = context.ownerDocument.createElement("span");
        element.textContent = source;
        return element;
    },
    async renderMermaid(source, context) {
        const element = context.ownerDocument.createElement("div");
        element.textContent = source;
        return element;
    },
    resolveImageSource: (source) => source,
    async saveClipboardAssets() {
        return [];
    },
});

export const mountTestEditor = (mode: "source" | "visual") => {
    const parent = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({
        parent,
        state: EditorState.create({
            doc: "line one\nline two",
            extensions: [history(), createSiyuanMarkraExtension({
                adapter: createTestHostAdapter(),
                documentPath: () => "/appearance-test.md",
                mode,
            })],
        }),
    });
    return {
        view,
        destroy: () => {
            view.destroy();
            parent.remove();
        },
    };
};

const appearanceSourcePaths = [
    "src/assets/scss/business/_markdown.scss",
    "src/markdown/markra-core/codemirror/theme.ts",
    "src/markdown/markra-core/codemirror/block-drag.ts",
    "src/markdown/markra-core/codemirror/callout-preview.ts",
    "src/markdown/markra-core/codemirror/clipboard-assets.ts",
    "src/markdown/markra-core/codemirror/code-block.ts",
    "src/markdown/markra-core/codemirror/fold-toggle.ts",
    "src/markdown/markra-core/codemirror/footnote-preview.ts",
    "src/markdown/markra-core/codemirror/horizontal-rule.ts",
    "src/markdown/markra-core/codemirror/image.ts",
    "src/markdown/markra-core/codemirror/math-preview.ts",
    "src/markdown/markra-core/codemirror/raw-html-preview.ts",
    "src/markdown/markra-core/codemirror/search.ts",
    "src/markdown/markra-core/codemirror/selection-hold.ts",
    "src/markdown/markra-core/codemirror/table-fragment-merge.ts",
    "src/markdown/markra-core/codemirror/table.ts",
    "src/markdown/markra-core/codemirror/trailing-space.ts",
    "src/markdown/markra-core/codemirror/typewriter.ts",
] as const;

export const readMarkdownAppearanceSources = () => appearanceSourcePaths.map((path) => ({
    path,
    text: readFileSync(resolve(process.cwd(), path), "utf8"),
}));

export const findIndependentBaseThemeDeclarations = (selector: string) => {
    const anchor = selector.includes(":not(")
        ? selector
        : selector.match(/\.[a-z][\w-]*/iu)?.[0] ?? selector;
    const escaped = anchor.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    const visualProperty = /\b(?:background(?:Color)?|border(?:Color|Radius)?|boxShadow|color|font(?:Family|Size|Style|Weight)?|height|lineHeight|margin|opacity|padding|width)\s*:/u;
    const declaration = new RegExp(`["'][^"']*${escaped}[^"']*["']\\s*:\\s*\\{([^}]*)\\}`, "gu");
    return readMarkdownAppearanceSources().filter((file) => file.path.endsWith(".ts")).flatMap((file) =>
        [...file.text.matchAll(declaration)]
            .filter((match) => visualProperty.test(match[1]))
            .map((match) => ({path: file.path, declaration: match[0]}))
    );
};

export const collectVisibleMarkraSelectors = () => [...new Set(readMarkdownAppearanceSources().flatMap((file) =>
    [...file.text.matchAll(/["'](\.(?:cm-)?markra-[\w-]+)/gu)].map((match) => match[1])
))].sort();

export const isSelectorCoveredByContract = (
    selector: string,
    contracts: readonly MarkdownAppearanceContract[],
) => contracts.some((contract) =>
    contract.markdownSelector.includes(selector) || contract.ownedSelectors.includes(selector)
);
