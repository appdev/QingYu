import {fetchPost} from "../../util/fetch";
import {escapeHtml} from "../../util/escape";
import {isMobile} from "../../util/functions";
import {upDownHint} from "../../util/upDownHint";
import {Menu} from "../../plugin/Menu";
import {hasClosestByClassName} from "../util/hasClosest";

interface TagMenuOptions {
    target: HTMLElement;
    getCurrentTags(): string[];
    toggleTag(tag: string, done: () => void): void;
}

export const openTagMenu = (options: TagMenuOptions) => {
    window.siyuan.menus.menu.remove();
    const menu = new Menu();
    menu.addItem({
        iconHTML: "",
        type: "empty",
        label: `<div class="fn__flex-column b3-menu__filter">
    <input class="b3-text-field fn__flex-shrink" placeholder="${window.siyuan.languages.tag}"/>
    <div class="fn__hr"></div>
    <div class="b3-list fn__flex-1 b3-list--background">
        <img style="margin: 0 auto;display: block;width: 64px;height: 64px" src="/stage/loading-pure.svg">
    </div>
</div>`,
        bind: (element) => {
            const listElement = element.querySelector(".b3-list--background") as HTMLElement;
            const inputElement = element.querySelector("input") as HTMLInputElement;
            const render = (keyword: string) => {
                fetchPost("/api/search/searchTag", {k: keyword}, (response) => {
                    let searchHTML = "";
                    let hasKey = false;
                    const currentTags = options.getCurrentTags();
                    response.data.tags.forEach((item: string, index: number) => {
                        const tag = Lute.UnEscapeHTMLStr(item.replace(/<mark>/g, "").replace(/<\/mark>/g, ""));
                        searchHTML += `<div class="b3-list-item b3-list-item--narrow${index === 0 ? " b3-list-item--focus" : ""}">
    <div class="fn__flex-1">${item}</div>
    ${currentTags.includes(tag) ? '<svg class="b3-menu__checked"><use xlink:href="#iconSelect"></use></svg>' : ""}
</div>`;
                        if (item === `<mark>${response.data.k}</mark>`) hasKey = true;
                    });
                    if (!hasKey && response.data.k) {
                        searchHTML = `<div data-type="new" class="b3-list-item b3-list-item--narrow${searchHTML ? "" : " b3-list-item--focus"}"><div class="fn__flex-1">${window.siyuan.languages.new} <mark>${escapeHtml(response.data.k)}</mark></div></div>${searchHTML}`;
                    }
                    listElement.innerHTML = searchHTML;
                });
            };
            const toggle = (item: HTMLElement | null, fallback = "") => {
                const tag = item
                    ? item.dataset.type === "new"
                        ? item.querySelector("mark")?.textContent.trim() || ""
                        : item.textContent.trim()
                    : fallback.trim();
                if (!tag) return;
                options.toggleTag(tag, () => {
                    inputElement.value = "";
                    render("");
                    inputElement.focus();
                });
            };
            inputElement.addEventListener("keydown", (event: KeyboardEvent) => {
                event.stopPropagation();
                if (event.isComposing) return;
                upDownHint(listElement, event);
                if (event.key === "Enter") {
                    toggle(listElement.querySelector(".b3-list-item--focus"), inputElement.value);
                } else if (event.key === "Escape") {
                    window.siyuan.menus.menu.remove();
                }
            });
            inputElement.addEventListener("input", (event) => {
                event.stopPropagation();
                render(inputElement.value.trim());
            });
            listElement.addEventListener("click", (event) => {
                const item = hasClosestByClassName(event.target as HTMLElement, "b3-list-item");
                if (item) toggle(item);
            });
            render("");
        }
    });
    const itemsElement = menu.element.querySelector(".b3-menu__items") as HTMLElement;
    itemsElement.style.overflow = "initial";
    if (isMobile()) {
        menu.fullscreen();
        (itemsElement.firstElementChild as HTMLElement).style.cssText = "padding: 0 8px;height: 100%;";
    } else {
        const rect = options.target.getBoundingClientRect();
        menu.open({x: rect.left, y: rect.bottom});
        (menu.element.querySelector("input") as HTMLInputElement).focus();
    }
};
