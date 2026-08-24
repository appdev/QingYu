import {escapeHtml} from "../util/escape";
import type {MarkdownOutlineItemWithPosition} from "./outlineModel";
import {buildMarkdownOutlineTreeData, getMarkdownOutlinePosition} from "./outlineModel";

export class MarkdownOutlineView {
    private filter = "";
    private items: readonly MarkdownOutlineItemWithPosition[] = [];
    private readonly onInput = (event: Event) => {
        this.filter = (event.currentTarget as HTMLInputElement).value.trim().toLocaleLowerCase();
        this.render();
    };
    private readonly onHeaderClick: EventListener;
    private readonly onContentClick: EventListener;

    constructor(
        private readonly element: HTMLElement,
        labels: {filter: string; outline: string},
        navigate: (position: number) => void,
    ) {
        element.classList.add("markdown-outline", "fn__flex-column", "file-tree", "sy__outline", "dockPanel");
        element.innerHTML = `<div class="block__icons fn__hidescrollbar">
    <div class="block__logo fn__flex-1">${escapeHtml(labels.outline)}</div>
    <input class="b3-text-field search__label fn__none fn__size200" placeholder="${escapeHtml(labels.filter)}">
    <span data-type="search" class="block__icon ariaLabel" data-position="north" aria-label="${escapeHtml(labels.filter)}">
        <svg><use xlink:href="#iconFilter"></use></svg>
    </span>
    <span class="fn__space"></span>
    <span data-type="expand" class="block__icon ariaLabel" data-position="north" aria-label="${escapeHtml(window.siyuan.languages.expandAll || "")}">
        <svg><use xlink:href="#iconExpand"></use></svg>
    </span>
    <span class="fn__space"></span>
    <span data-type="collapse" class="block__icon ariaLabel" data-position="north" aria-label="${escapeHtml(window.siyuan.languages.foldAll || "")}">
        <svg><use xlink:href="#iconContract"></use></svg>
    </span>
</div>
<div class="markdown-outline__items fn__flex-1" style="padding: 3px 0 8px"></div>`;
        element.querySelector<HTMLInputElement>("input").addEventListener("input", this.onInput);
        this.onHeaderClick = (event) => {
            const action = (event.target as HTMLElement).closest<HTMLElement>("[data-type]");
            if (action?.dataset.type === "search") {
                const input = element.querySelector<HTMLInputElement>("input");
                input.classList.remove("fn__none");
                input.select();
            } else if (action?.dataset.type === "expand") {
                element.querySelectorAll("ul.fn__none").forEach((item) => item.classList.remove("fn__none"));
                element.querySelectorAll(".b3-list-item__arrow").forEach((item) => item.classList.add("b3-list-item__arrow--open"));
            } else if (action?.dataset.type === "collapse") {
                element.querySelectorAll(".markdown-outline__items ul ul").forEach((item) => item.classList.add("fn__none"));
                element.querySelectorAll(".b3-list-item__arrow").forEach((item) => item.classList.remove("b3-list-item__arrow--open"));
            }
        };
        this.onContentClick = (event) => {
            const target = event.target as HTMLElement;
            const item = target.closest<HTMLElement>(".b3-list-item[data-node-id]");
            if (!item) return;
            if (target.closest(".b3-list-item__toggle") && item.nextElementSibling?.tagName === "UL") {
                item.querySelector(".b3-list-item__arrow")?.classList.toggle("b3-list-item__arrow--open");
                item.nextElementSibling.classList.toggle("fn__none");
            } else {
                const position = getMarkdownOutlinePosition(item.dataset.nodeId);
                if (position !== undefined) navigate(position);
            }
        };
        element.firstElementChild.addEventListener("click", this.onHeaderClick);
        element.lastElementChild.addEventListener("click", this.onContentClick);
    }

    update(items: readonly MarkdownOutlineItemWithPosition[]) {
        this.items = items;
        this.render();
    }

    destroy() {
        this.element.querySelector<HTMLInputElement>("input")?.removeEventListener("input", this.onInput);
        this.element.firstElementChild?.removeEventListener("click", this.onHeaderClick);
        this.element.lastElementChild?.removeEventListener("click", this.onContentClick);
    }

    private render() {
        const filterTree = (items: IBlockTree[]): IBlockTree[] => items.flatMap((item) => {
            const children = filterTree(item.children || []);
            if (!this.filter || item.name.toLocaleLowerCase().includes(this.filter) || children.length) {
                return [{...item, count: children.length, children}];
            }
            return [];
        });
        const renderTree = (items: IBlockTree[], root = false): string => {
            if (!items.length) {
                return root ? `<ul class="b3-list b3-list--background" role="tree"><li class="b3-list--empty">${escapeHtml(window.siyuan.languages.emptyContent || "")}</li></ul>` : "";
            }
            return `<ul${root ? ' class="b3-list b3-list--background" role="tree"' : ' role="group"'}>${items.map((item) => {
                const children = renderTree(item.children || []);
                const hasChildren = Boolean(children);
                const position = getMarkdownOutlinePosition(item.id);
                return `<li class="b3-list-item b3-list-item--hide-action" data-node-id="${item.id}" data-position="${position}" data-treetype="outline" data-type="NodeHeading" data-subtype="${item.subType}" role="treeitem" aria-level="${item.subType.slice(1)}" style="--file-toggle-width:${item.depth === 0 ? 22 : ((item.depth + 1) * 18)}px">
    <span style="padding-left: ${(item.depth * 18) || 4}px;margin-right: 2px" class="b3-list-item__toggle${hasChildren ? " b3-list-item__toggle--hl" : " fn__hidden"}">
        <svg class="b3-list-item__arrow${hasChildren ? " b3-list-item__arrow--open" : ""}"><use xlink:href="#iconRight"></use></svg>
    </span>
    <svg class="b3-list-item__graphic" style="height: 22px;width: 16px"><use xlink:href="#iconH${item.subType.slice(1)}"></use></svg>
    <span class="b3-list-item__text ariaLabel" data-position="parentE">${item.name}</span>
    ${item.count ? `<span class="counter">${item.count}</span>` : ""}
</li>${children}`;
            }).join("")}</ul>`;
        };
        this.element.querySelector<HTMLElement>(".markdown-outline__items").innerHTML =
            renderTree(filterTree(buildMarkdownOutlineTreeData(this.items)), true);
    }
}
