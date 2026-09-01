import {fetchSyncPost} from "../util/fetch";
import {processRender} from "../protyle/util/processCode";
import {highlightRender} from "../protyle/render/highlightRender";
import {addScript} from "../protyle/util/addScript";
import {Constants} from "../constants";
import {
    notebookRootPreviewCanvasOptions,
    notebookRootPreviewCaptureRootStyle,
    notebookRootPreviewCaptureStyle,
} from "./rules";
import {pruneDocumentCardPreviewContent} from "./previewPrune";
import {normalizeDocumentCardPreviewAssets} from "./previewAssets";

export interface PreviewRenderInput {
    reference: {kind: "sy" | "markdown", notebook: string, path: string, id: string};
    size: "medium";
}

const settlePreviewAssets = async (element: HTMLElement) => {
    const images = Array.from(element.querySelectorAll("img"));
    const imagePromises = images.map((image) => image.complete ? Promise.resolve() : new Promise<void>((resolve) => {
        image.addEventListener("load", () => resolve(), {once: true});
        image.addEventListener("error", () => resolve(), {once: true});
    }));
    const fonts = document.fonts?.ready || Promise.resolve();
    await Promise.race([
        Promise.all([fonts, ...imagePromises]),
        new Promise<void>((resolve) => setTimeout(resolve, 3000)),
    ]);
};

const canvasWebP = (canvas: HTMLCanvasElement, quality = 0.82) => new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("empty card preview")), "image/webp", quality);
});

export const renderDocumentCardPreview = async (input: PreviewRenderInput): Promise<Blob> => {
    const markdown = input.reference.kind === "markdown";
    const response = await fetchSyncPost(markdown ? "/api/export/exportMarkdownPreview" : "/api/export/exportPreviewHTML",
        markdown ? {notebook: input.reference.notebook, path: input.reference.path, image: true, keepFold: false} :
            {id: input.reference.id, image: true, keepFold: false});
    if (response.code !== 0) throw new Error(response.msg || "preview export failed");

    const captureRoot = document.createElement("div");
    captureRoot.setAttribute("aria-hidden", "true");
    captureRoot.style.cssText = notebookRootPreviewCaptureRootStyle();
    const host = document.createElement("div");
    host.className = "notebook-root__capture";
    host.dataset.themeMode = window.siyuan.config.appearance.mode === 1 ? "dark" : "light";
    const appearance = getComputedStyle(document.documentElement);
    const backgroundColor = appearance.getPropertyValue("--b3-theme-background").trim() ||
        (window.siyuan.config.appearance.mode === 1 ? "#1e1e1e" : "#ffffff");
    const foregroundColor = appearance.getPropertyValue("--b3-theme-on-background").trim();
    host.style.cssText = notebookRootPreviewCaptureStyle(backgroundColor, foregroundColor);
    const content = document.createElement("div");
    content.className = "protyle-wysiwyg";
    content.innerHTML = response.data.content;
    normalizeDocumentCardPreviewAssets(content);
    content.setAttribute("data-doc-type", response.data.type || "NodeDocument");
    Object.entries(response.data.attrs || {}).forEach(([key, value]) => content.setAttribute(key, value as string));
    host.append(content);
    captureRoot.append(host);
    document.body.append(captureRoot);
    try {
        pruneDocumentCardPreviewContent(content, host);
        processRender(content);
        highlightRender(content);
        await settlePreviewAssets(content);
        pruneDocumentCardPreviewContent(content, host);
        await addScript(`${Constants.PROTYLE_CDN}/js/html-to-image.min.js?v=1.11.13`, "protyleHtml2image");
        const medium = await window.htmlToImage.toCanvas(host, notebookRootPreviewCanvasOptions(backgroundColor));
        return canvasWebP(medium);
    } finally {
        captureRoot.remove();
    }
};
