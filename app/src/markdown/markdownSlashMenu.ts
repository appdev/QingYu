import type {EditorView} from "@codemirror/view";
import {
    closeMarkraSlashMenu,
    getMarkraSlashMenuState,
    runMarkraSlashMenuAction,
    type MarkraSlashMenuState,
} from "./markra-core/codemirror";

const commandIcons: Record<string, string> = {
    "block.paragraph": "iconParagraph",
    "block.bullet-list": "iconList",
    "block.task-list": "iconCheck",
    "block.ordered-list": "iconOrderedList",
    "block.quote": "iconQuote",
    "block.callout": "iconInfo",
    "block.code": "iconCode",
    "block.table": "iconTable",
    "insert.today": "iconCalendar",
};

const menuMargin = 12;
const menuOffset = 8;
const menuMaximumHeight = 320;

const commandIcon = (command: string) => command.startsWith("block.heading.")
    ? "iconHeadings"
    : commandIcons[command] || "iconParagraph";

export class MarkdownSlashMenuController {
    private element?: HTMLElement;
    private items?: HTMLElement;
    private renderedActions?: string;
    private destroyed = false;

    constructor(private view: EditorView, private scrollElement: HTMLElement) {
        this.scrollElement.addEventListener("scroll", this.updatePosition);
        window.addEventListener("resize", this.updatePosition);
        this.view.dom.ownerDocument.addEventListener("pointerdown", this.handleOutsidePointer, true);
    }

    public update() {
        if (this.destroyed) {
            return;
        }
        const menu = getMarkraSlashMenuState(this.view);
        if (!menu.open) {
            this.removeElement();
            return;
        }
        const coordinates = menu.to === null ? null : this.view.coordsAtPos(menu.to);
        if (!coordinates) {
            this.removeElement();
            return;
        }
        this.render(menu);
        this.position(coordinates);
        this.scrollSelectionIntoView();
    }

    public close() {
        if (!this.destroyed) {
            closeMarkraSlashMenu(this.view);
        }
        this.removeElement();
    }

    public destroy() {
        if (this.destroyed) {
            return;
        }
        this.destroyed = true;
        this.removeElement();
        this.scrollElement.removeEventListener("scroll", this.updatePosition);
        window.removeEventListener("resize", this.updatePosition);
        this.view.dom.ownerDocument.removeEventListener("pointerdown", this.handleOutsidePointer, true);
    }

    private render(menu: MarkraSlashMenuState) {
        const ownerDocument = this.view.dom.ownerDocument;
        const items = this.ensureElement();
        const renderedActions = JSON.stringify(menu.actions.map((action) => [action.command, action.label]));

        if (renderedActions !== this.renderedActions) {
            items.replaceChildren();
            items.scrollTop = 0;
            if (menu.actions.length === 0) {
                const empty = this.view.dom.ownerDocument.createElement("div");
                empty.className = "markdown-editor__slash-menu-empty";
                empty.textContent = window.siyuan?.languages?.emptyContent ?? "No matching commands";
                items.appendChild(empty);
            } else {
                menu.actions.forEach((action) => {
                    const button = ownerDocument.createElement("button");
                    button.className = "b3-menu__item";
                    button.dataset.command = action.command;
                    button.setAttribute("role", "menuitem");
                    button.type = "button";

                    const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
                    icon.classList.add("b3-menu__icon");
                    const use = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "use");
                    use.setAttribute("href", `#${commandIcon(action.command)}`);
                    icon.appendChild(use);
                    const label = ownerDocument.createElement("span");
                    label.className = "b3-menu__label";
                    label.textContent = action.label;
                    button.append(icon, label);
                    button.addEventListener("mousedown", (event) => event.preventDefault());
                    button.addEventListener("click", () => {
                        runMarkraSlashMenuAction(this.view, action.command);
                        this.update();
                    });
                    items.appendChild(button);
                });
            }
            this.renderedActions = renderedActions;
        }

        items.querySelectorAll<HTMLElement>(".b3-menu__item").forEach((item, index) => {
            const selected = index === menu.selectedIndex;
            item.classList.toggle("b3-menu__item--current", selected);
            item.setAttribute("aria-selected", String(selected));
        });
    }

    private ensureElement() {
        if (this.element && this.items) {
            return this.items;
        }
        const ownerDocument = this.view.dom.ownerDocument;
        const element = ownerDocument.createElement("div");
        element.className = "b3-menu markdown-editor__slash-menu";
        element.dataset.markdownSlashMenu = "true";
        element.setAttribute("role", "menu");
        element.setAttribute("aria-label", window.siyuan?.languages?.insert ?? "Insert block");
        const items = ownerDocument.createElement("div");
        items.className = "b3-menu__items";
        element.appendChild(items);
        ownerDocument.body.appendChild(element);
        this.element = element;
        this.items = items;
        return items;
    }

    private scrollSelectionIntoView() {
        const selected = this.items?.querySelector<HTMLElement>(".b3-menu__item--current");
        if (!selected || !this.items?.clientHeight) {
            return;
        }
        const selectedTop = selected.offsetTop;
        const selectedBottom = selectedTop + selected.offsetHeight;
        if (selectedTop < this.items.scrollTop) {
            this.items.scrollTop = selectedTop;
        } else if (selectedBottom > this.items.scrollTop + this.items.clientHeight) {
            this.items.scrollTop = selectedBottom - this.items.clientHeight;
        }
    }

    private position(coordinates: {bottom: number, left: number}) {
        if (!this.element) {
            return;
        }
        const rect = this.element.getBoundingClientRect();
        const maxLeft = Math.max(menuMargin, window.innerWidth - rect.width - menuMargin);
        const maxTop = Math.max(menuMargin, window.innerHeight - Math.min(rect.height, menuMaximumHeight) - menuMargin);
        const left = Math.max(menuMargin, Math.min(coordinates.left, maxLeft));
        const top = Math.max(menuMargin, Math.min(coordinates.bottom + menuOffset, maxTop));
        this.element.style.left = `${left}px`;
        this.element.style.top = `${top}px`;
    }

    private updatePosition = () => {
        if (!this.element) {
            return;
        }
        const menu = getMarkraSlashMenuState(this.view);
        const coordinates = menu.open && menu.to !== null ? this.view.coordsAtPos(menu.to) : null;
        if (!coordinates) {
            this.removeElement();
            return;
        }
        this.position(coordinates);
    };

    private handleOutsidePointer = (event: Event) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target?.closest('[data-markdown-slash-menu="true"]')) {
            return;
        }
        this.close();
    };

    private removeElement() {
        this.element?.remove();
        this.element = undefined;
        this.items = undefined;
        this.renderedActions = undefined;
    }
}
