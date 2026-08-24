export const refreshMarkdownEditorsForConfigMessage = (
    action: string,
    editors: readonly {refreshEditorConfig(): void}[],
) => {
    if (action !== "readonly" && action !== "setConf") return false;
    editors.forEach((editor) => editor.refreshEditorConfig());
    return true;
};
