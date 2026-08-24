export interface MarkdownEditorPreferences {
    codeIndentation: string;
    fullWidth: boolean;
    justify: boolean;
    rtl: boolean;
    spellcheck: boolean;
}

export const readMarkdownEditorPreferences = (
    editorConfig?: Record<string, unknown>,
): MarkdownEditorPreferences => {
    const config = editorConfig || window.siyuan?.config?.editor as unknown as Record<string, unknown> || {};
    const codeTabSpaces = Number(config.codeTabSpaces);
    return {
        codeIndentation: Number.isFinite(codeTabSpaces) && codeTabSpaces > 0 ? " ".repeat(Math.floor(codeTabSpaces)) : "\t",
        fullWidth: Boolean(config.fullWidth),
        justify: Boolean(config.justify),
        rtl: Boolean(config.rtl),
        spellcheck: Boolean(config.spellcheck),
    };
};

export const applyMarkdownEditorShellPreferences = (
    element: HTMLElement,
    titleElement: HTMLElement,
    preferences: MarkdownEditorPreferences,
) => {
    element.classList.toggle("markdown-editor--full-width", preferences.fullWidth);
    element.classList.toggle("markdown-editor--rtl", preferences.rtl);
    element.classList.toggle("markdown-editor--justify", preferences.justify);
    titleElement.spellcheck = preferences.spellcheck;
};

export const getMarkdownFontZoomSize = (
    event: Pick<WheelEvent, "ctrlKey" | "deltaX" | "deltaY" | "metaKey">,
    currentSize: number,
    enabled: boolean,
    macOS: boolean,
) => {
    if (!enabled || event.deltaX !== 0 || (macOS ? !event.metaKey : !event.ctrlKey)) return null;
    const delta = event.deltaY < 0 ? 1 : event.deltaY > 0 ? -1 : 0;
    const nextSize = Math.max(9, Math.min(72, currentSize + delta));
    return delta && nextSize !== currentSize ? nextSize : null;
};
