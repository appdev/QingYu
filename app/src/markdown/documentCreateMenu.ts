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
    createNative: (app: T, notebookId: string, parentPath: string) => unknown;
    createMarkdown: (app: T, notebookId: string, parentPath: string) => Promise<boolean>;
}

export const getDocumentCreateMenuItems = <T>(options: DocumentCreateMenuOptions<T>) => {
    const items: DocumentCreateMenuItem[] = [{
        id: "newDocument",
        label: options.newFileLabel,
        icon: "iconAddDoc",
        click: () => {
            options.createNative(options.app, options.notebookId, options.parentPath);
        },
    }];
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
