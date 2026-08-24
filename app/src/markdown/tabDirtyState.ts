export interface MarkdownDirtyModel {
    hasUnsavedChanges(): boolean;
}

export interface ProtyleDirtyModel {
    editor?: {protyle?: {updated?: boolean}};
}

export const isTabModelUnmodified = (
    model: unknown,
    isProtyleEditor: (value: unknown) => value is ProtyleDirtyModel,
    isMarkdownEditor: (value: unknown) => value is MarkdownDirtyModel,
) => {
    if (!model) return true;
    if (isProtyleEditor(model)) return !model.editor?.protyle?.updated;
    if (isMarkdownEditor(model)) return !model.hasUnsavedChanges();
    return true;
};
