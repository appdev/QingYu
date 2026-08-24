import type {EditorView, ViewUpdate} from "@codemirror/view";
import {
    findCodeMirrorSearchMatches,
    replaceAllCodeMirrorSearchMatches,
    replaceCodeMirrorSearchMatch,
    scrollCodeMirrorSearchMatchIntoView,
    updateCodeMirrorSearchDecorations,
} from "./markra-core/codemirror";
import {escapeHtml} from "../util/escape";

export class MarkdownSearchController {
    private readonly element: HTMLElement;
    private readonly query: HTMLInputElement;
    private readonly replacement: HTMLInputElement;
    private readonly caseSensitive: HTMLInputElement;
    private matches: {from: number; to: number}[] = [];
    private activeIndex = -1;
    private openState = false;

    constructor(private readonly view: EditorView, container: HTMLElement) {
        this.element = document.createElement("div");
        this.element.className = "markdown-editor__search fn__none";
        this.element.innerHTML = `<input class="b3-text-field" data-type="query" aria-label="${escapeHtml(window.siyuan.languages.search)}">
<label class="markdown-editor__search-case"><input type="checkbox" data-type="case">Aa</label>
<input class="b3-text-field fn__none" data-type="replacement" aria-label="${escapeHtml(window.siyuan.languages.replace)}">
<button class="block__icon" data-type="previous" aria-label="${escapeHtml(window.siyuan.languages.previous)}">↑</button>
<button class="block__icon" data-type="next" aria-label="${escapeHtml(window.siyuan.languages.next)}">↓</button>
<button class="block__icon fn__none" data-type="replace">${window.siyuan.languages.replace}</button>
<button class="block__icon fn__none" data-type="replace-all">${window.siyuan.languages.replaceAll}</button>
<button class="block__icon" data-type="close" aria-label="${escapeHtml(window.siyuan.languages.close)}">×</button>`;
        container.append(this.element);
        this.query = this.element.querySelector('[data-type="query"]');
        this.replacement = this.element.querySelector('[data-type="replacement"]');
        this.caseSensitive = this.element.querySelector('[data-type="case"]');
        this.query.addEventListener("input", () => this.refresh());
        this.caseSensitive.addEventListener("change", () => this.refresh());
        this.element.addEventListener("click", (event) => {
            const action = (event.target as HTMLElement).closest<HTMLElement>("[data-type]")?.dataset.type;
            if (action === "previous") this.next(-1);
            else if (action === "next") this.next(1);
            else if (action === "replace") this.replaceCurrent();
            else if (action === "replace-all") this.replaceAll();
            else if (action === "close") this.close();
        });
        this.element.addEventListener("keydown", (event) => {
            if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                this.close();
            } else if (event.key === "Enter") {
                event.preventDefault();
                this.next(event.shiftKey ? -1 : 1);
            }
        });
    }

    open(replace: boolean) {
        this.openState = true;
        this.element.classList.remove("fn__none");
        this.replacement.classList.toggle("fn__none", !replace);
        this.element.querySelectorAll('[data-type="replace"], [data-type="replace-all"]')
            .forEach((item) => item.classList.toggle("fn__none", !replace));
        const selection = this.view.state.selection.main;
        if (!selection.empty) this.query.value = this.view.state.sliceDoc(selection.from, selection.to);
        this.refresh();
        this.query.focus();
        this.query.select();
    }

    close() {
        if (!this.openState) return;
        this.openState = false;
        this.element.classList.add("fn__none");
        this.matches = [];
        this.activeIndex = -1;
        updateCodeMirrorSearchDecorations(this.view, [], -1);
        this.view.focus();
    }

    next(direction: 1 | -1) {
        if (!this.matches.length) return;
        this.activeIndex = (this.activeIndex + direction + this.matches.length) % this.matches.length;
        updateCodeMirrorSearchDecorations(this.view, this.matches, this.activeIndex);
        scrollCodeMirrorSearchMatchIntoView(this.view, this.matches[this.activeIndex]);
    }

    replaceCurrent() {
        const replaced = replaceCodeMirrorSearchMatch(this.view, this.matches[this.activeIndex], this.replacement.value);
        if (replaced) this.refresh();
        return replaced;
    }

    replaceAll() {
        const replaced = replaceAllCodeMirrorSearchMatches(this.view, this.matches, this.replacement.value);
        if (replaced) this.refresh();
        return replaced;
    }

    refresh() {
        if (!this.openState) return;
        this.matches = findCodeMirrorSearchMatches(this.view.state, this.query.value, {caseSensitive: this.caseSensitive.checked});
        this.activeIndex = this.matches.length ? Math.min(Math.max(this.activeIndex, 0), this.matches.length - 1) : -1;
        updateCodeMirrorSearchDecorations(this.view, this.matches, this.activeIndex);
    }

    refreshAfterViewUpdate(update: Pick<ViewUpdate, "docChanged">) {
        if (update.docChanged) this.refresh();
    }

    destroy() {
        this.openState = false;
        this.matches = [];
        this.activeIndex = -1;
        updateCodeMirrorSearchDecorations(this.view, [], -1);
        this.element.remove();
    }
}
