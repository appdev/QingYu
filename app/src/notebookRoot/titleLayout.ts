const pixelValue = (value: string) => Number.parseFloat(value) || 0;

export const notebookRootTitleLineCount = (
    cardHeight: number,
    previewMinimumHeight: number,
    fixedHeaderHeight: number,
    titleLineHeight: number,
) => {
    if (titleLineHeight <= 0) return 1;
    return Math.max(1, Math.floor((cardHeight - previewMinimumHeight - fixedHeaderHeight) / titleLineHeight));
};

export const updateNotebookRootTitleLayout = (root: HTMLElement) => {
    root.querySelectorAll<HTMLElement>(
        ".notebook-root__document--large, .notebook-root__document--masonry",
    ).forEach((card) => {
        const header = card.querySelector<HTMLElement>(".notebook-root__paper-header");
        const title = header?.querySelector<HTMLElement>(".notebook-root__document-title");
        const preview = card.querySelector<HTMLElement>(".notebook-root__preview-box");
        if (!header || !title || !preview || card.clientHeight <= 0) return;

        const headerStyle = getComputedStyle(header);
        const titleStyle = getComputedStyle(title);
        const lineHeight = pixelValue(titleStyle.lineHeight) || pixelValue(titleStyle.fontSize) * 1.25;
        const fixedHeaderHeight = pixelValue(headerStyle.paddingTop) + pixelValue(headerStyle.paddingBottom);
        const lines = notebookRootTitleLineCount(
            card.clientHeight,
            pixelValue(getComputedStyle(preview).minHeight),
            fixedHeaderHeight,
            lineHeight,
        );
        title.style.setProperty("--notebook-title-lines", lines.toString());
    });
};
