interface IMarkdownKeyboardEvent {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
}

export const isMarkdownSelectAll = (event: IMarkdownKeyboardEvent) => {
    return (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "a";
};

export const selectElementContents = (element: HTMLElement) => {
    const selection = element.ownerDocument.defaultView?.getSelection();
    if (!selection) {
        return;
    }
    const range = element.ownerDocument.createRange();
    range.selectNodeContents(element);
    selection.removeAllRanges();
    selection.addRange(range);
};

export const selectActiveEditorContent = () => {
    const activeElement = document.activeElement as HTMLElement;
    if (!activeElement) {
        return;
    }
    if (activeElement.classList.contains("protyle-title__input")) {
        selectElementContents(activeElement);
        return;
    }
    activeElement.dispatchEvent(new KeyboardEvent("keydown", {
        key: "a",
        code: "KeyA",
        metaKey: true,
        bubbles: true,
        cancelable: true,
    }));
    if (activeElement instanceof HTMLInputElement || activeElement instanceof HTMLTextAreaElement) {
        if (activeElement.selectionStart === activeElement.selectionEnd) {
            activeElement.select();
        }
        return;
    }
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed) {
        document.execCommand("selectAll");
    }
};
