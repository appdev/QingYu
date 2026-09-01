import {Dialog} from "../../dialog";
import {confirmDialog} from "../../dialog/confirmDialog";
import {hideMessage, showMessage} from "../../dialog/message";
import {fetchPost} from "../../util/fetch";
import {replaceFileName, replaceLocalPath} from "../../editor/rename";
import {escapeHtml} from "../../util/escape";
/// #if !BROWSER
import {ipcRenderer} from "electron";
import * as path from "path";
import {Constants} from "../../constants";
/// #endif
import {saveExportFile} from "../../protyle/util/compatibility";
import {exportMarkdownImage} from "../../protyle/export/util";
import {onExport} from "../../protyle/export";
/// #if !BROWSER
import {renderMarkdownPDF} from "../../protyle/export";
/// #endif
/// #if MOBILE
import {isInAndroid, isInHarmony, isInIOS} from "../../protyle/util/compatibility";
/// #endif
import {flushMarkdownEditors, markdownEditorRegistry} from "../markdownEditorRegistry";
import type {MarkdownExportFormat, MarkdownExportReference} from "./menu";

const pandocFormats = new Set<MarkdownExportFormat>([
    "rst", "asciidoc", "textile", "opml", "org", "mediawiki", "odt", "rtf", "epub",
]);

const request = (url: string, body: Record<string, unknown>) => new Promise<IWebSocketData>((resolve) => {
    fetchPost(url, body, (response) => resolve(response));
});

export const flushMarkdownBeforeExport = async (reference: MarkdownExportReference) => {
    const key = `workspace:${reference.notebook}:${reference.path}`;
    const editors = markdownEditorRegistry.getAll(key);
    return flushMarkdownEditors(editors);
};

const warnMissing = (response: IWebSocketData) => {
    const missing = response.data?.missing as string[] | undefined;
    if (missing?.length) showMessage(`${window.siyuan.languages.missingAssets}: ${missing.length}`, 5000, "warning");
};

const saveArtifact = async (url: string, reference: MarkdownExportReference, body: Record<string, unknown> = {}) => {
    const msgId = showMessage(window.siyuan.languages.exporting, -1);
    const response = await request(url, {...reference, ...body});
    if (response.code !== 0) {
        hideMessage(msgId);
        showMessage(response.msg, 0, "error");
        return;
    }
    await saveExportFile(response.data.zip, msgId);
    warnMissing(response);
};

const exportTemplate = (reference: MarkdownExportReference) => {
    const dialog = new Dialog({
        title: window.siyuan.languages.fileName,
        content: `<div class="b3-dialog__content"><input class="b3-text-field fn__block" value="${escapeHtml(replaceFileName((reference.path.split("/").pop() || "").replace(/\.(?:md|markdown)$/iu, "")))}"></div>
<div class="b3-dialog__action"><button class="b3-button b3-button--cancel">${window.siyuan.languages.cancel}</button><div class="fn__space"></div><button class="b3-button b3-button--text">${window.siyuan.languages.confirm}</button></div>`,
        width: "520px",
    });
    const input = dialog.element.querySelector("input") as HTMLInputElement;
    const buttons = dialog.element.querySelectorAll<HTMLButtonElement>("button");
    buttons[0].addEventListener("click", () => dialog.destroy());
    buttons[1].addEventListener("click", async () => {
        const name = replaceFileName(input.value.trim() || window.siyuan.languages.untitled);
        const save = async (overwrite: boolean) => request("/api/template/markdownSaveAsTemplate", {...reference, name, overwrite});
        const response = await save(false);
        if (response.code === 1) {
            confirmDialog(window.siyuan.languages.export, window.siyuan.languages.exportTplTip, async () => {
                const overwritten = await save(true);
                if (overwritten.code === 0) showMessage(window.siyuan.languages.exportTplSucc);
            });
        } else if (response.code === 0) {
            showMessage(window.siyuan.languages.exportTplSucc);
        } else {
            showMessage(response.msg, 0, "error");
        }
        dialog.destroy();
    });
};

const exportHTML = async (reference: MarkdownExportReference) => {
    /// #if !BROWSER
    const preview = await request("/api/export/exportMarkdownPreview", {...reference});
    if (preview.code !== 0) {
        showMessage(preview.msg, 0, "error");
        return;
    }
    const selected = await ipcRenderer.invoke(Constants.SIYUAN_GET, {
        cmd: "showOpenDialog",
        title: `${window.siyuan.languages.export} HTML (Markdown)`,
        properties: ["createDirectory", "openDirectory"],
    });
    if (selected.canceled) return;
    const savePath = path.join(selected.filePaths[0], replaceLocalPath(preview.data.name));
    const desktopMsgId = showMessage(window.siyuan.languages.exporting, -1);
    const desktopResponse = await request("/api/export/exportMarkdownHTML", {...reference, savePath});
    if (desktopResponse.code !== 0) {
        hideMessage(desktopMsgId);
        showMessage(desktopResponse.msg, 0, "error");
        return;
    }
    await onExport(desktopResponse, savePath, "", {type: "htmlmd", id: ""}, desktopMsgId);
    warnMissing(desktopResponse);
    return;
    /// #else
    const browserMsgId = showMessage(window.siyuan.languages.exporting, -1);
    const browserResponse = await request("/api/export/exportMarkdownHTML", {...reference, savePath: ""});
    if (browserResponse.code !== 0) {
        hideMessage(browserMsgId);
        showMessage(browserResponse.msg, 0, "error");
        return;
    }
    const html = await onExport(browserResponse, undefined, "", {type: "htmlmd", id: ""});
    const zipped = await request("/api/export/exportBrowserHTML", {
        folder: browserResponse.data.folder,
        html,
        name: browserResponse.data.name,
    });
    if (zipped.code !== 0) {
        hideMessage(browserMsgId);
        showMessage(zipped.msg, 0, "error");
        return;
    }
    await saveExportFile(zipped.data.zip, browserMsgId);
    warnMissing(browserResponse);
    /// #endif
};

const exportPDF = async (reference: MarkdownExportReference) => {
    /// #if !BROWSER
    renderMarkdownPDF(reference);
    /// #else
    const msgId = showMessage(window.siyuan.languages.exporting, -1);
    const response = await request("/api/export/exportMarkdownPreview", {...reference});
    if (response.code !== 0) {
        hideMessage(msgId);
        showMessage(response.msg, 0, "error");
        return;
    }
    const servePath = `${window.location.protocol}//${window.location.host}/`;
    const html = await onExport(response, undefined, servePath, {type: "pdf", id: ""});
    /// #if MOBILE
    if (isInAndroid()) {
        window.JSAndroid.print(response.data.name, html);
    } else if (isInHarmony()) {
        window.JSHarmony.print(response.data.name, html);
    } else if (isInIOS()) {
        window.webkit.messageHandlers.print.postMessage(response.data.name + "\u200b" + html);
    }
    /// #endif
    setTimeout(() => hideMessage(msgId), 3000);
    /// #endif
};

export const executeMarkdownExport = async (format: MarkdownExportFormat, reference: MarkdownExportReference) => {
    if (!await flushMarkdownBeforeExport(reference)) {
        showMessage(window.siyuan.languages._kernel[14].replace("%s", "Markdown"), 0, "error");
        return;
    }
    if (format === "template") {
        exportTemplate(reference);
    } else if (format === "markdownZip") {
        await saveArtifact("/api/export/exportMarkdownDocumentZip", reference);
    } else if (format === "image") {
        exportMarkdownImage(reference);
    } else if (format === "html") {
        await exportHTML(reference);
    } else if (format === "docx") {
        await saveArtifact("/api/export/exportMarkdownDocx", reference);
    } else if (format === "pdf") {
        await exportPDF(reference);
    } else if (pandocFormats.has(format)) {
        await saveArtifact("/api/export/exportMarkdownPandoc", reference, {format});
    }
};
