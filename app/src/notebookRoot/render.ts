import {escapeAttr, escapeHtml} from "../util/escape";
import type {NotebookRootDocument, NotebookRootListing, NotebookRootView} from "./types";
import {notebookRootDocumentKey} from "./documentKey";
import {
    formatNotebookRootUpdated,
    notebookRootCardLayout,
    notebookRootCardRatio,
    notebookRootTimeGroup,
    notebookRootTimeGroupField,
} from "./rules";

const placeholder = () => `<div class="notebook-root__placeholder" aria-hidden="true">
    <span class="notebook-root__preview-brand">${escapeHtml(window.siyuan.languages.siyuanNote || "QingYu")}</span>
    <span class="notebook-root__preview-status">${escapeHtml(window.siyuan.languages.notebookRootPreviewFailed || "预览不可用")}</span>
</div>`;

const timeGroupHeading = (label: string) =>
    `<div class="notebook-root__time-group" role="heading" aria-level="2">${escapeHtml(label)}</div>`;

export const renderNotebookRootDocuments = (
    documents: NotebookRootDocument[],
    view: NotebookRootView,
    listing: Pick<NotebookRootListing, "sortMode">,
    nowMilliseconds = Date.now(),
) => {
    if (documents.length === 0) {
        return `<div class="notebook-root__empty">${escapeHtml(window.siyuan.languages.notebookRootEmpty || "根目录中没有文档")}</div>`;
    }
    const locale = window.siyuan.config.appearance.lang;
    const groupField = view === "masonry" ? undefined : notebookRootTimeGroupField(listing.sortMode);
    const groupLabels = {
        today: window.siyuan.languages._kernel["261"] || "今天",
        yesterday: window.siyuan.languages._kernel["260"] || "昨天",
        past7Days: window.siyuan.languages.notebookRootPast7Days || "过去 7 天",
        past30Days: window.siyuan.languages.notebookRootPast30Days || "过去 30 天",
    };
    let previousGroupKey: string | undefined;
    const renderedDocuments = documents.map((document) => {
        let heading = "";
        if (groupField) {
            const group = notebookRootTimeGroup(document[groupField], nowMilliseconds, locale, groupLabels);
            if (!group) {
                previousGroupKey = undefined;
            } else if (group.key !== previousGroupKey) {
                heading = timeGroupHeading(group.label);
                previousGroupKey = group.key;
            }
        }
        const ratio = notebookRootCardRatio(view, document.cardRatio);
        const layout = notebookRootCardLayout(view);
        const previewKey = notebookRootDocumentKey({
            kind: document.kind,
            notebook: document.notebook,
            id: document.documentID,
            path: document.path,
        });
        const sharedAttributes = `tabindex="0" data-kind="${escapeAttr(document.kind)}" data-notebook="${escapeAttr(document.notebook)}"
 data-path="${escapeAttr(document.path)}" data-id="${escapeAttr(document.documentID)}"
 data-preview-key="${escapeAttr(previewKey)}"
 data-identity-conflict="${document.identityConflict}"
 data-identity-state="${escapeAttr(document.identityState)}" data-revision="${escapeAttr(document.revision)}"
 data-updated="${document.updated}" data-source-size="${document.size}"
 data-card-ratio="${ratio}" data-preview-state="loading" style="--notebook-card-ratio:${ratio}"`;
        if (layout === "list") {
            const updated = formatNotebookRootUpdated(
                document.updated,
                nowMilliseconds,
                window.siyuan.config.appearance.lang,
            );
            const created = formatNotebookRootUpdated(
                document.created,
                nowMilliseconds,
                window.siyuan.config.appearance.lang,
            );
            return heading + `<article class="notebook-root__document notebook-root__document--list" ${sharedAttributes}>
    <div class="notebook-root__list-name">
        <div class="notebook-root__preview-box">${placeholder()}</div>
        <div class="notebook-root__list-copy">
            <span class="notebook-root__document-title">${escapeHtml(document.title)}</span>
            ${document.previewText ? `<span class="notebook-root__document-preview-text">${escapeHtml(document.previewText)}</span>` : ""}
        </div>
    </div>
    <time class="notebook-root__list-time">${escapeHtml(updated)}</time>
    <time class="notebook-root__list-time">${escapeHtml(created)}</time>
</article>`;
        }
        return heading + `<article class="notebook-root__document notebook-root__document--${view}" ${sharedAttributes}>
    <div class="notebook-root__paper-header">
        <div class="notebook-root__document-title">${escapeHtml(document.title)}</div>
    </div>
    <div class="notebook-root__preview-box">
        ${placeholder()}
        <div class="notebook-root__image-fader" aria-hidden="true"></div>
    </div>
</article>`;
    }).join("");
    if (view !== "list") {
        return renderedDocuments;
    }
    return `<div class="notebook-root__list-header" role="row">
    <span>${escapeHtml(window.siyuan.languages.docName || "文档名")}</span>
    <span>${escapeHtml(window.siyuan.languages.updatedTime || "更新时间")}</span>
    <span>${escapeHtml(window.siyuan.languages.createdTime || "创建时间")}</span>
</div>${renderedDocuments}`;
};
