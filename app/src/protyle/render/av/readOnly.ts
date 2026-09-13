const readers = new WeakSet<HTMLElement>();

const editingControls = [
    "av-add", "av-add-more", "av-add-template", "av-add-bottom", "av-add-top", "av-header-add",
    "av-header-more", "av-row-more", "av-gallery-edit", "av-gallery-more", "av-more", "av-filter",
    "av-sort", "av-switcher", "addColumn", "remove",
].map(type => `[data-type="${type}"]`).join(",");

// 数据库只保留阅读交互，后续分页和虚拟滚动插入的节点也适用。
export const bindDatabaseReadOnly = (element: HTMLElement, onClick?: (event: MouseEvent) => void) => {
    const sanitize = () => {
        if (element.getAttribute("contenteditable") !== "false") element.setAttribute("contenteditable", "false");
        element.querySelectorAll(editingControls).forEach(item => item.remove());
        element.querySelectorAll('[data-type="editCol"]').forEach(item => item.removeAttribute("data-type"));
        element.querySelectorAll(".av__cursor, .av__widthdrag, .av__drag-fill, .av__gallery-actions, .av__gallery-add").forEach(item => item.remove());
        element.querySelectorAll('[draggable="true"]').forEach(item => item.setAttribute("draggable", "false"));
        element.querySelectorAll('[contenteditable]:not([contenteditable="false"])').forEach(item => {
            if (item.getAttribute("data-type") !== "av-search") item.setAttribute("contenteditable", "false");
        });
        element.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea").forEach(item => {
            if (!item.readOnly) item.readOnly = true;
            if (item.type === "checkbox" && !item.disabled) item.disabled = true;
        });
    };
    sanitize();
    if (readers.has(element)) return;
    readers.add(element);
    const isSearch = (event: Event) => (event.target as Element)?.closest?.('[data-type="av-search"]');
    for (const type of ["beforeinput", "paste", "cut", "drop", "dragstart", "change"]) {
        element.addEventListener(type, event => {
            if (isSearch(event)) return;
            event.preventDefault();
            event.stopImmediatePropagation();
        }, true);
    }
    for (const type of ["mousedown", "dblclick", "contextmenu", "keydown", "keyup", "input"]) {
        element.addEventListener(type, event => {
            if (!isSearch(event)) event.stopImmediatePropagation();
        }, true);
    }
    element.addEventListener("click", event => {
        onClick?.(event);
        const target = event.target as Element;
        if (target.closest('a, [data-type="block-ref"], [data-type="av-backlinks-toggle"], [data-type="av-backlink-open"]')) return;
        event.stopImmediatePropagation();
    }, true);
    new MutationObserver(sanitize).observe(element, {childList: true, subtree: true, attributes: true,
        attributeFilter: ["contenteditable", "draggable"]});
};
