export interface MarkdownTitleTransactionOptions {
    applyTitle(title: string): boolean;
    flush(): Promise<boolean>;
    metadataTitle: string;
    previousTitle: string;
    rename(): Promise<boolean>;
    renameRequired: boolean;
}

export const runMarkdownTitleTransaction = async (options: MarkdownTitleTransactionOptions) => {
    if (!options.applyTitle(options.metadataTitle) || !await options.flush()) return false;
    if (!options.renameRequired) return true;
    try {
        if (await options.rename()) return true;
    } catch {
        // 重命名失败时继续恢复 Front Matter 标题。
    }
    if (!options.applyTitle(options.previousTitle)) return false;
    await options.flush();
    return false;
};
