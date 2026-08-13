import type {App} from "../index";
import {showMessage} from "../dialog/message";
import {openLink} from "../editor/openLink";
import {processRender} from "../protyle/util/processCode";
import {setPosition} from "../util/setPosition";
import {mountCodeLanguageMenu, type CodeLanguageFilterDetail} from "../protyle/codeLanguageMenu";
import {
    type MarkdownClipboardAssetRequest,
    type MarkdownHostAdapter,
    type MarkdownIconName,
    type MarkdownRenderContext,
} from "./markra-core/adapter";
import {createSiyuanMarkdownIcon} from "./markra-core/shared";
import {convertSiyuanClipboardHtmlToMarkdown} from "./luteHtmlConverter";
import {mountSiyuanMarkdownPopover} from "./siyuanMarkdownPopover";

export {mountSiyuanMarkdownPopover} from "./siyuanMarkdownPopover";

export interface SiyuanMarkdownAdapterOptions {
    app: App;
    documentPath(): string;
}

const normalizeCodeLanguages = (value: unknown) => [...new Set((Array.isArray(value) ? value : [])
    .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    .map((item) => item.trim()))];

const emitCodeLanguageUpdate = (app: App, detail: CodeLanguageFilterDetail) => {
    detail.languages = normalizeCodeLanguages(detail.languages);
    app.plugins.forEach((plugin) => {
        try {
            plugin.eventBus.emit("code-language-update", detail);
        } catch (error) {
            console.warn("code-language-update failed", error);
        }
        detail.languages = normalizeCodeLanguages(detail.languages);
    });
    return detail.languages;
};

const iconName = (name: MarkdownIconName) => {
    if (name === "trash") {
        return "trash" as const;
    }
    if (name === "remove" || name === "zoomOut") {
        return "remove" as const;
    }
    return "add" as const;
};

export const resolveMarkdownImageSource = (source: string) => {
    const value = source.trim().replace(/^<|>$/gu, "");
    if (!value || /^(?:javascript|vbscript):/iu.test(value)) {
        return null;
    }
    if (/^data:/iu.test(value)) {
        return /^data:image\/(?:avif|gif|jpeg|png|svg\+xml|webp);/iu.test(value) ? value : null;
    }
    if (/^(?:https?|blob):/iu.test(value)) {
        try {
            return new URL(value).toString();
        } catch (error) {
            return null;
        }
    }
    if (Array.from(value).some((character) => character.charCodeAt(0) < 32) || value.includes("\\")) {
        return null;
    }
    return value.startsWith("assets/") ? `/${value}` : value;
};

export const resolveMarkdownCoverSource = (source: string) => {
    const value = source.trim().replace(/^<|>$/gu, "");
    if (/^[a-z][a-z\d+.-]*:/iu.test(value) && !/^(?:https?|blob|data):/iu.test(value)) return null;
    return resolveMarkdownImageSource(value);
};

export const uploadMarkdownAssets = async ({files}: MarkdownClipboardAssetRequest) => {
    if (files.length === 0) {
        return [];
    }
    const form = new FormData();
    files.forEach((file) => form.append("file[]", file));
    form.append("assetsDirPath", "/assets/");
    const response = await fetch("/api/asset/upload", {body: form, method: "POST"});
    if (!response.ok) {
        throw new Error(`Asset upload failed with HTTP ${response.status}`);
    }
    const result = await response.json();
    if (result.code !== 0) {
        throw new Error(result.msg || "Asset upload failed");
    }
    const successful = result.data?.succMap || {};
    return Object.keys(successful).sort((left, right) => left.localeCompare(right, undefined, {numeric: true}))
        .map((name) => ({markdownDestination: successful[name], name}));
};

const renderWithSiyuan = (source: string, subtype: "math" | "mermaid", context: MarkdownRenderContext) => {
    const element = context.ownerDocument.createElement("div");
    element.className = "render-node";
    element.dataset.content = source;
    element.dataset.subtype = subtype;
    const target = context.ownerDocument.createElement("div");
    target.setAttribute("spin", "1");
    element.append(target);
    processRender(element);
    return element;
};

export const createSiyuanMarkdownAdapter = (
    options: SiyuanMarkdownAdapterOptions,
): MarkdownHostAdapter => ({
    convertHtmlToMarkdown: convertSiyuanClipboardHtmlToMarkdown,
    createIcon(name, className, ownerDocument) {
        return createSiyuanMarkdownIcon(ownerDocument, iconName(name), className);
    },
    notifyError(message) {
        showMessage(message, 6000, "error");
    },
    mountPopover(request) {
        return mountSiyuanMarkdownPopover({
            ...request,
            position(anchor, popover) {
                const rect = anchor.getBoundingClientRect();
                setPosition(popover, rect.left, rect.bottom, rect.height);
            },
        });
    },
    openCodeLanguageMenu(request) {
        return mountCodeLanguageMenu({
            anchor: request.anchor,
            container: request.anchor.closest(".markdown-editor") ?? request.ownerDocument.body,
            currentLanguage: request.currentLanguage,
            languages: request.languages,
            labels: {
                clear: window.siyuan.languages.clear,
                search: window.siyuan.languages.search,
            },
            onDestroy: request.onDestroy,
            onFilter: (detail) => emitCodeLanguageUpdate(options.app, detail),
            onSelect: request.onSelect,
            position: (anchor, popover) => {
                const rect = anchor.getBoundingClientRect();
                setPosition(popover, rect.left, rect.bottom, rect.height);
            },
        });
    },
    openLink(target) {
        openLink(options.app, target);
    },
    positionPopover(anchor, popover) {
        const anchorRect = anchor.getBoundingClientRect();
        setPosition(popover, anchorRect.left, anchorRect.bottom, anchorRect.height);
    },
    renderMath(source, _displayMode, context) {
        return renderWithSiyuan(source, "math", context);
    },
    async renderMermaid(source, context) {
        return renderWithSiyuan(source, "mermaid", context);
    },
    resolveImageSource(source) {
        return resolveMarkdownImageSource(source);
    },
    saveClipboardAssets: uploadMarkdownAssets,
    updateCodeLanguages(detail) {
        return emitCodeLanguageUpdate(options.app, detail);
    },
});
