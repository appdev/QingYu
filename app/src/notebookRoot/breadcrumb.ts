import {escapeAttr, escapeHtml} from "../util/escape";

export const renderNotebookRootBreadcrumbItem = (notebookId: string, notebookName: string) =>
    `<button type="button" class="protyle-breadcrumb__item protyle-breadcrumb__item--notebook-root" data-type="notebook-root" data-notebook-id="${escapeAttr(notebookId)}">
    <span class="protyle-breadcrumb__text">${escapeHtml(notebookName)}</span>
</button>`;
