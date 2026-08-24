export interface ExternalMarkdownCloseable {
    flushForExit(): Promise<boolean>;
    discardChanges?(): void;
}

interface ExternalMarkdownTransferable {
    prepareClose(): Promise<boolean>;
    releaseExternalCapability(): Promise<void>;
}

export const prepareExternalMarkdownEditorTransfer = async (editor: ExternalMarkdownTransferable) => {
    if (!await editor.prepareClose()) return false;
    try {
        await editor.releaseExternalCapability();
        return true;
    } catch {
        return false;
    }
};

export const prepareExternalMarkdownEditorsForExit = async (
    editors: readonly ExternalMarkdownCloseable[],
) => {
    for (const editor of editors) {
        if (!await editor.flushForExit()) return false;
    }
    return true;
};

export const discardAllExternalMarkdownChanges = (editors: readonly ExternalMarkdownCloseable[]) => {
    editors.forEach((editor) => editor.discardChanges?.());
};
