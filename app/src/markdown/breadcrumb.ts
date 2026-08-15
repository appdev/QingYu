import {escapeHtml} from "../util/escape";

export const renderMarkdownBreadcrumb = (parts: string[]) => parts.map((item, index) => {
    const activeClass = index === parts.length - 1 ? " protyle-breadcrumb__item--active" : "";
    return `<span class="protyle-breadcrumb__item${activeClass}">
    <span class="protyle-breadcrumb__text">${escapeHtml(item)}</span>
</span>`;
}).join('<svg class="protyle-breadcrumb__arrow"><use xlink:href="#iconRight"></use></svg>');
