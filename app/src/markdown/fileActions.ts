import {Dialog} from "../dialog";
import {App} from "../index";
import {Constants} from "../constants";
import {isMobile} from "../util/functions";
import {fetchSyncPost} from "../util/fetch";
import {openMarkdownFile} from "../editor/util";
import {confirmDialog} from "../dialog/confirmDialog";
import {escapeHtml} from "../util/escape";
import {getAllModels} from "../layout/getAll";
import {MenuItem} from "../menus/Menu";
import {newFile, newFileInTree, getNewFilePath} from "../util/newFile";
import {isEncryptedBox} from "../util/pathName";
import {closeMobileMarkdownEditor, getMobileMarkdownEditor, openMobileMarkdownFile} from "../mobile/markdown";

const markdownNameDialog = (title: string, initialName: string, callback: (name: string) => Promise<boolean>) => {
    const dialog = new Dialog({
        title,
        content: `<div class="b3-dialog__content"><input class="b3-text-field fn__block" value=""></div>
<div class="b3-dialog__action">
    <button class="b3-button b3-button--cancel">${window.siyuan.languages.cancel}</button><div class="fn__space"></div>
    <button class="b3-button b3-button--text">${window.siyuan.languages.confirm}</button>
</div>`,
        width: isMobile() ? "92vw" : "520px",
    });
    dialog.element.setAttribute("data-key", Constants.DIALOG_RENAME);
    const inputElement = dialog.element.querySelector("input") as HTMLInputElement;
    const buttons = dialog.element.querySelectorAll<HTMLButtonElement>(".b3-button");
    inputElement.value = initialName;
    inputElement.focus();
    inputElement.select();
    dialog.bindInput(inputElement, () => buttons[1].click());
    buttons[0].addEventListener("click", () => dialog.destroy());
    buttons[1].addEventListener("click", async () => {
        const name = inputElement.value.trim();
        if (!name) {
            return;
        }
        if (await callback(name)) {
            dialog.destroy();
        }
    });
};

export const newMarkdownFile = async (app: App, notebookId?: string, parentPath?: string) => {
    if (!notebookId) {
        const target = getNewFilePath();
        notebookId = target.notebookId;
        parentPath = target.currentPath;
    }
    if (!notebookId || isEncryptedBox(notebookId)) {
        return false;
    }
    const response = await fetchSyncPost("/api/markdown/create", {
        notebook: notebookId,
        parentPath: parentPath || "/",
        name: `${window.siyuan.languages.untitled}.md`,
        autoName: true,
    });
    if (response.code !== 0) {
        return false;
    }
    if (isMobile()) {
        openMobileMarkdownFile(app, notebookId, response.data.path, response.data.name);
    } else {
        await openMarkdownFile(app, notebookId, response.data.path, response.data.name);
    }
    return true;
};

export const openNewFileMenu = (app: App, options: {
    notebookId?: string;
    currentPath?: string;
    position?: {x: number, y: number};
    mobile?: boolean;
} = {}) => {
    window.siyuan.menus.menu.remove();
    window.siyuan.menus.menu.append(new MenuItem({
        id: "newDocument",
        label: window.siyuan.languages.newFile,
        icon: "iconAddDoc",
        click: () => options.notebookId ?
            newFileInTree(app, options.notebookId, options.currentPath || "/") : newFile(app),
    }).element);
    if (!options.notebookId || !isEncryptedBox(options.notebookId)) {
        window.siyuan.menus.menu.append(new MenuItem({
            id: "newMarkdown",
            label: `${window.siyuan.languages.newFile} Markdown`,
            icon: "iconMarkdown",
            click: () => void newMarkdownFile(app, options.notebookId, options.currentPath),
        }).element);
    }
    if (options.mobile) {
        window.siyuan.menus.menu.fullscreen("bottom");
    } else {
        window.siyuan.menus.menu.popup(options.position);
    }
};

export const renameMarkdownFile = (notebookId: string, path: string) => {
    const currentName = path.substring(path.lastIndexOf("/") + 1);
    markdownNameDialog(window.siyuan.languages.rename, currentName, async (name) => {
        const editor = (isMobile() ? [getMobileMarkdownEditor()] : getAllModels().markdown)
            .find((item) => item?.notebookId === notebookId && item.path === path);
        if (editor) {
            return editor.rename(name);
        }
        const response = await fetchSyncPost("/api/markdown/rename", {notebook: notebookId, path, name});
        return response.code === 0;
    });
};

export const removeMarkdownFile = (notebookId: string, path: string) => {
    const name = path.substring(path.lastIndexOf("/") + 1);
    confirmDialog(window.siyuan.languages.deleteOpConfirm, `${window.siyuan.languages.confirmDelete} <b>${escapeHtml(name)}</b>?`, async () => {
        const response = await fetchSyncPost("/api/markdown/remove", {notebook: notebookId, path});
        if (response.code !== 0) {
            return;
        }
        const mobileEditor = getMobileMarkdownEditor();
        if (mobileEditor?.notebookId === notebookId && mobileEditor.path === path) {
            closeMobileMarkdownEditor();
        }
        getAllModels().markdown.filter((item) => item.notebookId === notebookId && item.path === path).forEach((item) => item.parent.close());
    });
};
