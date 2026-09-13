import {type EditorState} from "@codemirror/state";
import {type EditorView} from "@codemirror/view";
import {getSelectedImageAtomicRange} from "../markra-core/codemirror/image-atomic";

export type EditorMode = "source" | "visual";
export type EditAction = "copy" | "copy-plain" | "cut" | "delete" | "paste" | "paste-plain" | "paste-escaped" | "select-all";
export type FormatAction = "link" | "bold" | "italic" | "strike" | "highlight" | "code" | "math" | "clear" | "annotation";
export type ActionId = EditAction | FormatAction;
export type ActionResult = "done" | "disabled" | "stale" | "failed" | "cancelled";
export interface ActionHost {
    mode(): EditorMode;
    isAlive(): boolean;
    addAnnotation(): boolean;
    report(result: "stale" | "failed"): void;
    requestLink(initial: string): Promise<string | null>;
}
export interface SelectionContext {
    view: EditorView;
    mode: EditorMode;
    kind: "editor" | "table-cell" | "preview";
    target: HTMLElement;
    state: EditorState;
    from: number;
    to: number;
    domRange: Range | null;
    cellText: string | null;
    imageRange?: {from: number; to: number};
}

export const captureContext = (view: EditorView, target: EventTarget | null, mode: EditorMode): SelectionContext | null => {
    const element = target instanceof Element ? target : null;
    if (!element || !view.contentDOM.contains(element) || element.closest("input, textarea, button")) return null;
    const imageRange = getSelectedImageAtomicRange(view.state);
    if (imageRange && element.closest("img, .img")) {
        return {view, mode, kind: "preview", target: view.contentDOM, state: view.state, ...imageRange,
            domRange: null, cellText: null, imageRange};
    }
    const selection = view.dom.ownerDocument.getSelection();
    const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
    const cell = element.closest<HTMLElement>("td, th");
    const preview = element.closest<HTMLElement>('[contenteditable="false"]');
    const kind = cell ? "table-cell" : preview ? "preview" : "editor";
    const scope = cell || preview || view.contentDOM;
    if (kind !== "editor" && (!range || !scope.contains(range.startContainer) || !scope.contains(range.endContainer))) return null;
    return {view, mode, kind, target: scope, state: view.state,
        from: view.state.selection.main.from, to: view.state.selection.main.to,
        domRange: kind === "editor" ? null : range.cloneRange(), cellText: kind === "editor" ? null : scope.innerHTML};
};

export const isContextCurrent = (context: SelectionContext): boolean => {
    const {view, state, target, domRange} = context;
    if (!view.dom.isConnected || !target.isConnected || !view.state.doc.eq(state.doc) ||
        !view.state.selection.eq(state.selection)) return false;
    const mode = view.dom.getAttribute("data-markdown-mode");
    if (mode && mode !== context.mode) return false;
    if (context.imageRange) {
        const selected = getSelectedImageAtomicRange(view.state);
        if (!selected || selected.from !== context.from || selected.to !== context.to) return false;
    }
    if (domRange && (target.innerHTML !== context.cellText || !target.contains(domRange.startContainer) ||
        !target.contains(domRange.endContainer))) return false;
    const selection = target.ownerDocument.getSelection();
    if (domRange && selection?.rangeCount && view.contentDOM.contains(selection.anchorNode)) {
        const current = selection.getRangeAt(0);
        if (current.startContainer !== domRange.startContainer || current.startOffset !== domRange.startOffset ||
            current.endContainer !== domRange.endContainer || current.endOffset !== domRange.endOffset) return false;
    }
    return true;
};

export const hasSelection = (context: SelectionContext) => context.imageRange || context.kind === "editor"
    ? context.from < context.to : Boolean(context.domRange && !context.domRange.collapsed);

export const restoreContextSelection = (context: SelectionContext) => {
    if (context.kind === "editor") context.view.focus();
    else if (context.domRange) {
        context.target.focus();
        const selection = context.target.ownerDocument.getSelection();
        selection.removeAllRanges();
        selection.addRange(context.domRange.cloneRange());
    }
};
