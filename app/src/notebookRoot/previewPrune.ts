const previewVerticalOverscan = 160;

const removeFollowingElements = (parent: Element, firstHidden: Element) => {
    const lastChild = parent.lastElementChild;
    if (!lastChild) {
        return;
    }
    const range = document.createRange();
    range.setStartBefore(firstHidden);
    range.setEndAfter(lastChild);
    range.deleteContents();
};

const pruneElementChildren = (parent: Element, cutoff: number) => {
    for (const child of Array.from(parent.children)) {
        const bounds = child.getBoundingClientRect();
        if (bounds.top >= cutoff) {
            removeFollowingElements(parent, child);
            return;
        }
        if (bounds.bottom > cutoff && child.childElementCount > 0) {
            pruneElementChildren(child, cutoff);
        }
    }
};

export const pruneDocumentCardPreviewContent = (
    content: HTMLElement,
    captureElement: HTMLElement,
    overscan = previewVerticalOverscan,
) => {
    const cutoff = captureElement.getBoundingClientRect().bottom + overscan;
    pruneElementChildren(content, cutoff);
};
