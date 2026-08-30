export type MarkdownTitleSyncResult = "deferred" | "unchanged" | "updated";

export class MarkdownTitleComposition {
    private composing = false;

    public start() {
        this.composing = true;
    }

    public end() {
        this.composing = false;
    }

    public acceptsInput(event: Pick<InputEvent, "isComposing">) {
        return !this.composing && !event.isComposing;
    }

    public acceptsKeydown(event: Pick<KeyboardEvent, "isComposing">) {
        return !this.composing && !event.isComposing;
    }
}

export const isMarkdownTitleEditing = (element: HTMLElement) => {
    return element.ownerDocument.activeElement === element;
};

export const isAutoUntitledMarkdownName = (fileStem: string, untitled: string) => {
    const escaped = untitled.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    return new RegExp(`^${escaped}(?:\\s+\\d+)?$`, "u").test(fileStem);
};

export const isGeneratedUntitledMarkdownTitle = (
    fileStem: string,
    title: string | undefined,
    untitled: string,
) => isAutoUntitledMarkdownName(fileStem, untitled) && (!title || title === fileStem);

export const syncMarkdownTitleElement = (
    element: HTMLElement,
    title: string,
): MarkdownTitleSyncResult => {
    if (element.textContent === title) return "unchanged";
    // 活动标题 DOM 是用户输入源，异步保存结果只能更新模型，不能覆盖当前选区。
    if (isMarkdownTitleEditing(element)) return "deferred";
    element.textContent = title;
    return "updated";
};

export const syncMarkdownTitlePresentation = (
    element: HTMLElement,
    title: string,
    placeholder: string,
    empty: boolean,
) => {
    if (empty) {
        element.setAttribute("placeholder", placeholder);
        return syncMarkdownTitleElement(element, "");
    }
    element.removeAttribute("placeholder");
    return syncMarkdownTitleElement(element, title);
};

export const syncMarkdownTitleEditable = (element: HTMLElement, editable: boolean) => {
    const value = String(editable);
    if (element.getAttribute("contenteditable") === value) return false;
    element.setAttribute("contenteditable", value);
    return true;
};
