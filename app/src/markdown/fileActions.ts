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
import {movePathTo} from "../util/pathName";
import {showMessage} from "../dialog/message";
import {
    createMarkdownDocument,
    duplicateMarkdownDocument,
    flushMarkdownDocumentEditors,
    MarkdownDocumentReference,
    moveMarkdownDocument,
    recycleMarkdownDocument,
    renameMarkdownDocument,
} from "./documentManagement";
import {
    abortMarkdownMutationAcrossRenderers,
    commitMarkdownMutationAcrossRenderers,
    MarkdownManagementIPC,
    markdownCoordinatorEditor,
    prepareMarkdownMutationAcrossRenderers,
} from "./managementCoordinator";
/// #if !BROWSER
import {ipcRenderer} from "electron";
/// #endif

let markdownManagementIPC: MarkdownManagementIPC | undefined;
/// #if BROWSER
markdownManagementIPC = undefined;
/// #else
markdownManagementIPC = ipcRenderer;
/// #endif

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
    const editors = allMarkdownEditors();
    const reference = markdownRef(notebookId, parentPath || "/");
    const data = await createMarkdownDocument({
        notebook: notebookId,
        parentPath: parentPath || "/",
        name: `${window.siyuan.languages.untitled}.md`,
        autoName: true,
    }, {
        request: (url, body) => fetchSyncPost(url, body),
        prepareOperation: (operationID) => prepareMarkdownMutationAcrossRenderers(
            markdownManagementIPC,
            window.location.origin,
            reference,
            editors.map(markdownCoordinatorEditor),
            operationID,
            {mode: "barrier"},
        ),
        commitMutation: (operationID, lease, mutation) => commitMarkdownMutationAcrossRenderers(
            markdownManagementIPC,
            window.location.origin,
            operationID,
            lease,
            mutation,
            editors.map(markdownCoordinatorEditor),
        ),
        abortMutation: (operationID, lease) => abortMarkdownMutationAcrossRenderers(
            markdownManagementIPC,
            window.location.origin,
            operationID,
            lease,
        ),
    });
    if (!data) {
        return false;
    }
    if (isMobile()) {
        openMobileMarkdownFile(app, notebookId, data.path as string, data.name as string);
    } else {
        await openMarkdownFile(app, notebookId, data.path as string, data.name as string);
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
        const ref = markdownRef(notebookId, path);
        return renameMarkdownDocument(ref, name, markdownMutationDependencies(ref));
    });
};

const markdownRef = (notebook: string, path: string): MarkdownDocumentReference => ({kind: "markdown", notebook, path});

const allMarkdownEditors = () => Array.from(new Set([
    ...getAllModels().markdown,
    getMobileMarkdownEditor(),
].filter(Boolean)));

const markdownMutationDependencies = (ref: MarkdownDocumentReference) => {
    const editors = allMarkdownEditors();
    return {
        editors,
        request: (url: string, body: Record<string, unknown>) => fetchSyncPost(url, body),
        loadRevision: async () => {
            const response = await fetchSyncPost("/api/markdown/get", {notebook: ref.notebook, path: ref.path});
            return response.code === 0 ? response.data.revision as string : null;
        },
        prepareRevision: (reference: MarkdownDocumentReference, operationID: string) =>
            prepareMarkdownMutationAcrossRenderers(
                markdownManagementIPC,
                window.location.origin,
                reference,
                editors.map(markdownCoordinatorEditor),
                operationID,
            ),
        commitMutation: (operationID: string, lease: string, mutation: Parameters<
            typeof commitMarkdownMutationAcrossRenderers
        >[4]) => commitMarkdownMutationAcrossRenderers(
            markdownManagementIPC,
            window.location.origin,
            operationID,
            lease,
            mutation,
            editors.map(markdownCoordinatorEditor),
        ),
        abortMutation: (operationID: string, lease: string) => abortMarkdownMutationAcrossRenderers(
            markdownManagementIPC,
            window.location.origin,
            operationID,
            lease,
        ),
        migrate: (_from: MarkdownDocumentReference, to: MarkdownDocumentReference, revision: string) => {
            editors.filter((editor) => editor.notebookId === ref.notebook && editor.path === ref.path)
                .forEach((editor) => (editor as typeof editor & {
                    applyWorkspaceDocumentReference(notebook: string, path: string, revision: string): void;
                }).applyWorkspaceDocumentReference(to.notebook, to.path, revision));
        },
        close: () => {
            const mobileEditor = getMobileMarkdownEditor();
            if (mobileEditor?.notebookId === ref.notebook && mobileEditor.path === ref.path) closeMobileMarkdownEditor();
            getAllModels().markdown.filter((editor) => editor.notebookId === ref.notebook && editor.path === ref.path)
                .forEach((editor) => editor.parent.close());
        },
    };
};

export const duplicateMarkdownFile = async (notebookId: string, path: string) => {
    const ref = markdownRef(notebookId, path);
    if (!await duplicateMarkdownDocument(ref, markdownMutationDependencies(ref))) {
        showMessage(window.siyuan.languages.transactionError, 6000, "error");
    }
};

export const copyMarkdownContent = async (notebookId: string, path: string) => {
    const ref = markdownRef(notebookId, path);
    const editors = allMarkdownEditors().filter((editor) => editor.notebookId === notebookId && editor.path === path);
    if (editors.length > 0 && !await flushMarkdownDocumentEditors(ref, editors)) {
        showMessage(window.siyuan.languages.transactionError, 6000, "error");
        return null;
    }
    const response = await fetchSyncPost("/api/markdown/get", {notebook: notebookId, path});
    if (response.code !== 0) {
        showMessage(response.msg, 6000, "error");
        return null;
    }
    return response.data.content as string;
};

export const moveMarkdownFile = (notebookId: string, path: string) => {
    movePathTo({
        title: `${window.siyuan.languages.move} ${path.substring(path.lastIndexOf("/") + 1)}`,
        cb: async (toPath, toNotebook) => {
            const targetNotebook = toNotebook[0];
            const targetParentPath = toPath[0];
            const ref = markdownRef(notebookId, path);
            if (!await moveMarkdownDocument(ref, {notebook: targetNotebook, directory: targetParentPath},
                markdownMutationDependencies(ref))) {
                showMessage(window.siyuan.languages.transactionError, 6000, "error");
            }
        },
    });
};

export const moveMarkdownFileTo = async (
    notebookId: string,
    path: string,
    targetNotebook: string,
    targetParentPath: string,
) => {
    const ref = markdownRef(notebookId, path);
    return moveMarkdownDocument(ref, {notebook: targetNotebook, directory: targetParentPath}, markdownMutationDependencies(ref));
};

export const removeMarkdownFile = (notebookId: string, path: string) => {
    const name = path.substring(path.lastIndexOf("/") + 1);
    confirmDialog(window.siyuan.languages.deleteOpConfirm, `${window.siyuan.languages.confirmDelete} <b>${escapeHtml(name)}</b>?`, async () => {
        const ref = markdownRef(notebookId, path);
        if (!await recycleMarkdownDocument(ref, markdownMutationDependencies(ref))) {
            showMessage(window.siyuan.languages.transactionError, 6000, "error");
        }
    });
};
