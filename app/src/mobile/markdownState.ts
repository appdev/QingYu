interface IMobileMarkdownEditor {
    destroy: () => void;
    element: Pick<HTMLElement, "remove">;
    flush: () => Promise<boolean>;
    notebookId: string;
    path: string;
    duplicate?: () => Promise<import("../markdown/documentSource").MarkdownDocument | null>;
    move?: (toNotebook: string, toParentPath: string) => Promise<boolean>;
    rename: (name: string) => Promise<boolean>;
    refreshEditorConfig?: () => void;
}

interface IHiddenMobileElement {
    classList: Pick<DOMTokenList, "remove">;
}

let editor: IMobileMarkdownEditor;
let hiddenElements: IHiddenMobileElement[] = [];

export const getMobileMarkdownEditor = () => editor;

export const setMobileMarkdownEditor = (nextEditor: IMobileMarkdownEditor, nextHiddenElements: IHiddenMobileElement[]) => {
    editor = nextEditor;
    hiddenElements = nextHiddenElements;
};

export const closeMobileMarkdownEditor = () => {
    if (!editor) {
        return;
    }
    editor.destroy();
    editor.element.remove();
    editor = undefined;
    hiddenElements.forEach((item) => item.classList.remove("fn__none"));
    hiddenElements = [];
};

export const refreshMobileMarkdownReadOnly = () => {
    editor?.refreshEditorConfig?.();
};

export const closeMobileMarkdownEditorForNotebook = (notebookId: string) => {
    if (!editor || editor.notebookId !== notebookId) return false;
    closeMobileMarkdownEditor();
    return true;
};
