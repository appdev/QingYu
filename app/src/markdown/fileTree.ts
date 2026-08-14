export const getMarkdownFileTreeDisplayName = (name: string) => name.replace(/\.(?:md|markdown)$/iu, "");

export const getMarkdownFileTreeNames = (name: string) => ({
    dataName: name,
    displayName: getMarkdownFileTreeDisplayName(name),
});

type MarkdownFileTreeCreator<T> = (app: T, notebookId: string, parentPath: string) => Promise<boolean>;

export const createMarkdownFromFileTreeAction = <T>(
    app: T,
    actionElement: Element,
    createMarkdown: MarkdownFileTreeCreator<T>,
) => {
    const notebookId = actionElement?.closest("ul[data-url]")?.getAttribute("data-url");
    const parentPath = actionElement?.parentElement?.getAttribute("data-path");
    if (!notebookId || parentPath === null) {
        return false;
    }
    void createMarkdown(app, notebookId, parentPath);
    return true;
};
