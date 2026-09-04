import {escapeHtml} from "../util/escape";
import {renderNotebookRootBreadcrumbItem} from "../notebookRoot/breadcrumb";

export const renderMarkdownBreadcrumb = (parts: string[], notebookId?: string) => parts.map((item, index) => {
    if (index === 0 && notebookId) return renderNotebookRootBreadcrumbItem(notebookId, item);
    const activeClass = index === parts.length - 1 ? " protyle-breadcrumb__item--active" : "";
    return `<span class="protyle-breadcrumb__item${activeClass}">
    <span class="protyle-breadcrumb__text">${escapeHtml(item)}</span>
</span>`;
}).join('<svg class="protyle-breadcrumb__arrow"><use xlink:href="#iconRight"></use></svg>');
