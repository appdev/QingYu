import {StateEffect} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {captureContext, hasSelection, type ActionHost, type FormatAction} from "./context";
import {executeAction} from "./commands";
import {defaultKeyboardShortcuts, matchesKeyboardShortcutEvent, type KeyboardShortcutAction} from "../markra-core/shared/keyboard-shortcuts";
import {createSelectionMenu} from "./menu";
import {SelectionToolbar} from "./toolbar";

export const mountSelectionActions = (view: EditorView, host: ActionHost) => {
    const toolbar = new SelectionToolbar(view, host);
    let disposed = false;
    let refreshQueued = false;
    const contextmenu = (event: MouseEvent) => {
        const target = event.target instanceof HTMLElement ? event.target : null;
        if (!target || !view.contentDOM.contains(target) || target.closest("input, textarea, button")) return;
        const inWidget = target.closest('td, th, [contenteditable="false"]');
        if (!inWidget) {
            const position = view.posAtCoords({x: event.clientX, y: event.clientY});
            const range = view.state.selection.main;
            if (position !== null && (position < range.from || position > range.to || range.empty)) {
                view.dispatch({selection: {anchor: position}});
            }
        }
        const context = captureContext(view, target, host.mode());
        if (!context) return;
        event.preventDefault();
        event.stopPropagation();
        toolbar.hide();
        const menu = window.siyuan.menus.menu;
        menu.remove();
        createSelectionMenu(context, host).forEach((item) => menu.addItem(item));
        menu.popup({x: event.clientX, y: event.clientY});
    };
    const keydown = (event: KeyboardEvent) => {
        if (event.altKey && event.key === "F10") {
            event.preventDefault();
            event.stopPropagation();
            toolbar.focus();
            return;
        }
        if (event.isComposing || host.mode() !== "visual") return;
        const bindings: Partial<Record<KeyboardShortcutAction, FormatAction>> = {
            bold: "bold", italic: "italic", strikethrough: "strike", inlineCode: "code", link: "link",
        };
        const action = (Object.keys(bindings) as KeyboardShortcutAction[]).find((key) =>
            matchesKeyboardShortcutEvent(event, defaultKeyboardShortcuts[key]));
        if (!action) return;
        const context = captureContext(view, event.target, host.mode());
        if (!context || context.kind !== "editor" || !hasSelection(context)) return;
        event.preventDefault();
        event.stopPropagation();
        toolbar.hide();
        void executeAction(bindings[action], context, host);
    };
    view.contentDOM.addEventListener("contextmenu", contextmenu);
    view.contentDOM.addEventListener("keydown", keydown, true);
    view.dispatch({effects: StateEffect.appendConfig.of(EditorView.updateListener.of(() => {
        if (disposed || refreshQueued) return;
        refreshQueued = true;
        // 焦点切换时先等待目标按钮获得焦点，避免在 blur 中隐藏正在接收焦点的工具栏。
        queueMicrotask(() => {
            refreshQueued = false;
            if (!disposed) toolbar.refresh();
        });
    }))});
    return {refresh: () => toolbar.refresh(), destroy: () => {
        disposed = true;
        toolbar.destroy();
        view.contentDOM.removeEventListener("contextmenu", contextmenu);
        view.contentDOM.removeEventListener("keydown", keydown, true);
    }};
};
