import type {App} from "../index";
import {MenuItem} from "../menus/Menu";
import {copySubMenu, exportMd, movePathToMenu, renameMenu} from "../menus/commonMenuItem";
import {deleteFile} from "../editor/deleteFile";
import {
    copyMarkdownContent,
    duplicateMarkdownFile,
    moveMarkdownFile,
    removeMarkdownFile,
    renameMarkdownFile,
} from "../markdown/fileActions";
import {createMarkdownExportMenu} from "../markdown/export/menu";
import {executeMarkdownExport} from "../markdown/export/actions";
import {writeText} from "../protyle/util/compatibility";
import type {NotebookRootDocument} from "./types";
/// #if !BROWSER
import * as path from "path";
/// #endif

interface NotebookRootContextMenuOptions {
    app: App;
    document: NotebookRootDocument;
    position: IPosition;
    open: () => void;
}

const markdownCopyItems = (document: NotebookRootDocument, readonly: boolean): IMenu[] => {
    const items: IMenu[] = [{
        id: "copyRelativePath",
        label: window.siyuan.languages.copyRelativePath,
        click: () => writeText(document.path),
    }];
    /// #if !BROWSER
    items.push({
        id: "copyAbsolutePath",
        label: window.siyuan.languages.copyAbsolutePath,
        click: () => writeText(path.join(window.siyuan.config.system.dataDir, document.notebook, document.path)),
    });
    /// #endif
    items.push({
        id: "copyMarkdown",
        label: window.siyuan.languages.copyMarkdown,
        click: async () => {
            const content = await copyMarkdownContent(document.notebook, document.path);
            if (content !== null) writeText(content);
        },
    });
    if (!readonly) {
        items.push({
            id: "duplicateMarkdown",
            label: window.siyuan.languages.duplicate,
            click: () => void duplicateMarkdownFile(document.notebook, document.path),
        });
    }
    return items;
};

const appendMarkdownItems = (document: NotebookRootDocument) => {
    const menu = window.siyuan.menus.menu;
    const readonly = window.siyuan.config.readonly;
    if (!readonly) {
        menu.append(new MenuItem({
            id: "renameMarkdown",
            label: window.siyuan.languages.rename,
            icon: "iconEdit",
            click: () => renameMarkdownFile(document.notebook, document.path),
        }).element);
    }
    menu.append(new MenuItem({
        id: "copy",
        label: window.siyuan.languages.copy,
        type: "submenu",
        icon: "iconCopy",
        submenu: markdownCopyItems(document, readonly),
    }).element);
    if (!readonly) {
        menu.append(new MenuItem({
            id: "moveMarkdown",
            label: window.siyuan.languages.move,
            icon: "iconMove",
            click: () => moveMarkdownFile(document.notebook, document.path),
        }).element);
    }
    menu.append(new MenuItem(createMarkdownExportMenu({
        notebook: document.notebook,
        path: document.path,
    }, (format, reference) => void executeMarkdownExport(format, reference))).element);
    if (!readonly) {
        menu.append(new MenuItem({id: "separator_delete", type: "separator"}).element);
        menu.append(new MenuItem({
            id: "deleteMarkdown",
            label: window.siyuan.languages.delete,
            icon: "iconTrashcan",
            click: () => removeMarkdownFile(document.notebook, document.path),
        }).element);
    }
};

const appendNativeItems = (document: NotebookRootDocument) => {
    const menu = window.siyuan.menus.menu;
    const readonly = window.siyuan.config.readonly;
    if (!readonly) {
        menu.append(renameMenu({
            path: document.path,
            notebookId: document.notebook,
            name: document.title,
            type: "file",
            docId: document.documentID,
        }));
    }
    menu.append(new MenuItem({
        id: "copy",
        label: window.siyuan.languages.copy,
        type: "submenu",
        icon: "iconCopy",
        submenu: copySubMenu([document.documentID], false, undefined, document.documentID),
    }).element);
    if (!readonly) {
        menu.append(movePathToMenu([document.path]));
    }
    const exportItem = exportMd(document.documentID);
    if (exportItem) {
        menu.append(exportItem);
    }
    if (!readonly) {
        menu.append(new MenuItem({id: "separator_delete", type: "separator"}).element);
        menu.append(new MenuItem({
            id: "delete",
            label: window.siyuan.languages.delete,
            icon: "iconTrashcan",
            click: () => deleteFile(document.notebook, document.path),
        }).element);
    }
};

export const openNotebookRootContextMenu = (options: NotebookRootContextMenuOptions) => {
    const menu = window.siyuan.menus.menu;
    menu.remove();
    menu.append(new MenuItem({
        id: "openDocument",
        label: window.siyuan.languages.openDocument,
        icon: "iconOpen",
        click: options.open,
    }).element);
    if (options.document.kind === "markdown") {
        appendMarkdownItems(options.document);
    } else {
        appendNativeItems(options.document);
    }
    menu.popup(options.position);
};
