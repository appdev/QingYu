import {notebookRootElementKey} from "./documentKey";

export interface NotebookRootLayoutSnapshot {
    anchorKey?: string;
    anchorOffset: number;
    scrollTop: number;
    selectedKey?: string;
    images: Map<string, HTMLImageElement>;
}

export const captureNotebookRootLayoutSnapshot = (
    root: HTMLElement,
    documents: HTMLElement,
): NotebookRootLayoutSnapshot => {
    const rootRect = root.getBoundingClientRect();
    const visible = Array.from(documents.querySelectorAll<HTMLElement>(".notebook-root__document"))
        .map((element) => ({element, rect: element.getBoundingClientRect()}))
        .filter(({rect}) => rect.bottom > rootRect.top && rect.top < rootRect.bottom)
        .sort((a, b) => a.rect.top - b.rect.top || a.rect.left - b.rect.left);
    const anchor = visible[0];
    const images = new Map<string, HTMLImageElement>();
    documents.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((element) => {
        const image = element.querySelector<HTMLImageElement>(".notebook-root__preview");
        if (image) {
            images.set(notebookRootElementKey(element), image);
        }
    });
    const selected = documents.querySelector<HTMLElement>(".notebook-root__document--selected");
    return {
        anchorKey: anchor ? notebookRootElementKey(anchor.element) : undefined,
        anchorOffset: anchor ? anchor.rect.top - rootRect.top : 0,
        scrollTop: root.scrollTop,
        selectedKey: selected ? notebookRootElementKey(selected) : undefined,
        images,
    };
};

export const hydrateNotebookRootLayout = (
    documents: HTMLElement,
    snapshot: NotebookRootLayoutSnapshot,
) => {
    documents.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((element) => {
        const key = notebookRootElementKey(element);
        if (key === snapshot.selectedKey) {
            element.classList.add("notebook-root__document--selected");
        }
        const image = snapshot.images.get(key);
        const previewBox = element.querySelector<HTMLElement>(".notebook-root__preview-box");
        if (!image || !previewBox) return;
        previewBox.querySelector(".notebook-root__placeholder")?.remove();
        const fader = previewBox.querySelector(".notebook-root__image-fader");
        if (fader) {
            previewBox.insertBefore(image, fader);
        } else {
            previewBox.append(image);
        }
        element.dataset.previewReady = "true";
        element.dataset.previewState = "ready";
    });
};

export const restoreNotebookRootScrollAnchor = (
    root: HTMLElement,
    documents: HTMLElement,
    snapshot: NotebookRootLayoutSnapshot,
) => {
    const anchor = snapshot.anchorKey ?
        Array.from(documents.querySelectorAll<HTMLElement>(".notebook-root__document"))
            .find((element) => notebookRootElementKey(element) === snapshot.anchorKey) : undefined;
    if (!anchor) {
        root.scrollTop = snapshot.scrollTop;
        return;
    }
    const newOffset = anchor.getBoundingClientRect().top - root.getBoundingClientRect().top;
    root.scrollTop += newOffset - snapshot.anchorOffset;
};
