import {
    consumeNextPlainTextPaste,
    dispatchPlainTextPaste,
    markNextPlainTextPaste,
} from "./markra-core/plain-text-paste";

export type MarkdownEditorCommand = "search" | "replace" | "toggle-fullscreen" | "toggle-typewriter" |
    "toggle-rtl" | "toggle-justify" | "paste-plain-text" | "source-mode" | "visual-mode";

export interface MarkdownCommandTarget {
    element: HTMLElement;
    view: {contentDOM: HTMLElement};
    isReadOnly(): boolean;
    openSearch(replace: boolean): void;
    refreshEditorConfig(): void;
    setMode(mode: "source" | "visual"): void;
    toggleFullscreen(): void;
    toggleTypewriterMode(): void;
    updateEditorPreference(key: "justify" | "rtl", value: boolean): void;
}

interface MarkdownShortcutKeymap {
    general: {replace: {custom: string}; search: {custom: string}};
    editor: {general: {
        fullscreen: {custom: string};
        preview: {custom: string};
        rtl: {custom: string};
        wysiwyg: {custom: string};
    }};
}

export const resolveMarkdownShortcut = (
    event: KeyboardEvent,
    keymap: MarkdownShortcutKeymap,
    matches: (hotkey: string, event: KeyboardEvent) => boolean,
): MarkdownEditorCommand | null => {
    if (matches(keymap.general.replace.custom, event)) return "replace";
    if (matches(keymap.general.search.custom, event)) return "search";
    if (matches(keymap.editor.general.fullscreen.custom, event)) return "toggle-fullscreen";
    if (matches(keymap.editor.general.rtl.custom, event)) return "toggle-rtl";
    if (matches(keymap.editor.general.preview.custom, event) || matches(keymap.editor.general.wysiwyg.custom, event)) {
        return "visual-mode";
    }
    return null;
};

export const isMarkdownTypewriterShortcut = (event: KeyboardEvent) =>
    (event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "y";

export const routeMarkdownShortcut = (
    editor: MarkdownCommandTarget,
    event: KeyboardEvent,
    keymap: MarkdownShortcutKeymap,
    matches: (hotkey: string, event: KeyboardEvent) => boolean,
) => {
    const command = resolveMarkdownShortcut(event, keymap, matches);
    if (!command || !executeMarkdownEditorCommand(editor, command)) return false;
    event.preventDefault();
    event.stopPropagation();
    return true;
};

export const requestMarkdownPlainTextPaste = (
    editor: MarkdownCommandTarget,
    target: EventTarget | null | undefined,
    readText: () => string | null | undefined | Promise<string | null | undefined>,
) => {
    if (editor.isReadOnly() || !(target instanceof HTMLElement) ||
        !(target === editor.view.contentDOM || editor.view.contentDOM.contains(target)) ||
        !(document.activeElement === editor.view.contentDOM || editor.view.contentDOM.contains(document.activeElement))) {
        return false;
    }
    markNextPlainTextPaste(editor.view.contentDOM, "use-native-text");
    try {
        void Promise.resolve(readText()).then((text) => {
            if (!text || !editor.view.contentDOM.isConnected || editor.isReadOnly()) return;
            if (!consumeNextPlainTextPaste(editor.view.contentDOM)) return;
            dispatchPlainTextPaste(target, text);
        }).catch(() => undefined);
    } catch {
        // 原生 paste 事件仍可消费 pending intent 并读取 text/plain。
    }
    return false;
};

export const executeMarkdownEditorCommand = (
    editor: MarkdownCommandTarget,
    command: MarkdownEditorCommand,
    target?: EventTarget | null,
) => {
    if (command === "search" || command === "replace") {
        editor.openSearch(command === "replace");
        return true;
    }
    if (command === "source-mode" || command === "visual-mode") {
        editor.setMode(command === "source-mode" ? "source" : "visual");
        return true;
    }
    if (command === "toggle-fullscreen") {
        editor.toggleFullscreen();
        return true;
    }
    if (command === "toggle-typewriter" || command === "toggle-rtl" || command === "toggle-justify") {
        if (command === "toggle-typewriter") {
            editor.toggleTypewriterMode();
            return true;
        }
        const key = command === "toggle-rtl" ? "rtl" : "justify";
        editor.updateEditorPreference(key, !window.siyuan.config.editor[key]);
        return true;
    }
    if (command === "paste-plain-text") {
        return requestMarkdownPlainTextPaste(editor, target, () => navigator.clipboard?.readText());
    }
    return false;
};
