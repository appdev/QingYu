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
import {documentCardPreviewFontCSS} from "./previewFonts";
import {documentCardPreviewThemeSignature} from "./theme";
import {assertDocumentCardPreviewActive, queueDocumentCardPreviewRender} from "./previewRenderQueue";
import {captureDocumentCardPreview, documentCardPreviewFormat} from "./previewCapture";

export interface PreviewRenderInput {
    reference: {kind: "sy" | "markdown", notebook: string, path: string, id: string};
    size: "medium";
    shouldContinue?: () => boolean;
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

const canvasImage = (canvas: HTMLCanvasElement, quality = 0.82) => new Promise<Blob>((resolve, reject) => {
    const type = documentCardPreviewFormat() === "png" ? "image/png" : "image/webp";
    canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("empty card preview")), type, quality);
});

export const renderDocumentCardPreview = (input: PreviewRenderInput): Promise<Blob> =>
    queueDocumentCardPreviewRender(input.shouldContinue, () => renderPreview(input));

const renderPreview = async (input: PreviewRenderInput): Promise<Blob> => {
    const appearance = documentCardPreviewThemeSignature();
    const checkpoint = () => {
        assertDocumentCardPreviewActive(input.shouldContinue);
        assertDocumentCardPreviewActive(() => appearance === documentCardPreviewThemeSignature());
    };
    const markdown = input.reference.kind === "markdown";
    const response = await fetchSyncPost(markdown ? "/api/export/exportMarkdownPreview" : "/api/export/exportPreviewHTML",
        markdown ? {
            notebook: input.reference.notebook,
            path: input.reference.path,
            image: true,
            keepFold: false,
            cardPreview: true,
        } :
            {id: input.reference.id, image: true, keepFold: false});
    if (response.code !== 0) throw new Error(response.msg || "preview export failed");
    checkpoint();

    const captureRoot = document.createElement("div");
    captureRoot.setAttribute("aria-hidden", "true");
    captureRoot.style.cssText = notebookRootPreviewCaptureRootStyle();
    const host = document.createElement("div");
    host.className = "notebook-root__capture";
    host.dataset.themeMode = window.siyuan.config.appearance.mode === 1 ? "dark" : "light";
    const styles = getComputedStyle(document.documentElement);
    const backgroundColor = styles.getPropertyValue("--b3-theme-background").trim() ||
        (window.siyuan.config.appearance.mode === 1 ? "#1e1e1e" : "#ffffff");
    const foregroundColor = styles.getPropertyValue("--b3-theme-on-background").trim();
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
        checkpoint();
        pruneDocumentCardPreviewContent(content, host);
        // 嵌入页面等内容依赖原文档环境，继续使用原有转换流程，避免静默遗漏。
        const needsDocumentContext = content.querySelector("iframe, object, embed, video, style") ||
            Array.from(content.querySelectorAll("use")).some(use => {
                const href = use.getAttribute("href") || use.getAttribute("xlink:href");
                return !href?.startsWith("#") || !document.getElementById(href.slice(1));
            }) ||
            Array.from(content.querySelectorAll("*")).some(element => element.shadowRoot);
        if (documentCardPreviewFormat() === "png" && !needsDocumentContext) {
            const image = await captureDocumentCardPreview(host);
            checkpoint();
            return image;
        }
        await addScript(`${Constants.PROTYLE_CDN}/js/html-to-image.min.js?v=1.11.13`, "protyleHtml2image");
        checkpoint();
        const fontEmbedCSS = await documentCardPreviewFontCSS(host, appearance);
        checkpoint();
        const medium = await window.htmlToImage.toCanvas(host, {...notebookRootPreviewCanvasOptions(backgroundColor), fontEmbedCSS});
        checkpoint();
        return canvasImage(medium);
    } finally {
        captureRoot.remove();
    }
};
