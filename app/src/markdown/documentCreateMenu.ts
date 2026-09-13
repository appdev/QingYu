export interface DocumentCreateMenuItem {
    id: "newDocument" | "newMarkdown";
    label: string;
    icon: "iconAddDoc" | "iconMarkdown";
    click: () => void;
}

interface DocumentCreateMenuOptions<T> {
    app: T;
    notebookId: string;
    parentPath: string;
    newFileLabel: string;
    encrypted: boolean;
    createMarkdown: (app: T, notebookId: string, parentPath: string) => Promise<boolean>;
}

export const getDocumentCreateMenuItems = <T>(options: DocumentCreateMenuOptions<T>) => {
    const items: DocumentCreateMenuItem[] = [];
    if (!options.encrypted) {
        items.push({
            id: "newMarkdown",
            label: `${options.newFileLabel} Markdown`,
            icon: "iconMarkdown",
            click: () => {
                void options.createMarkdown(options.app, options.notebookId, options.parentPath);
            },
        });
    }
    return items;
};
