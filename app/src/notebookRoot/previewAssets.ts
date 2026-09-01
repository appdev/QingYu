const normalizeLocalAssetPath = (value: string) => {
    if (value.startsWith("assets/")) {
        return `/${value}`;
    }
    if (value.startsWith("./assets/")) {
        return value.slice(1);
    }
    return value;
};

export const normalizeDocumentCardPreviewAssets = (content: HTMLElement) => {
    ["src", "data-src", "poster"].forEach((attribute) => {
        content.querySelectorAll<HTMLElement>(`[${attribute}]`).forEach((element) => {
            const value = element.getAttribute(attribute);
            if (!value) {
                return;
            }
            const normalized = normalizeLocalAssetPath(value);
            if (normalized !== value) {
                element.setAttribute(attribute, normalized);
            }
        });
    });
};
