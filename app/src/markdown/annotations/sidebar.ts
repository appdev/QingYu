import {Transaction, type EditorState} from "@codemirror/state";
import {syntaxTree} from "@codemirror/language";
import {projectMarkdownRange} from "../textProjection";
import {EditorView, type ViewUpdate} from "@codemirror/view";
import {genUUID} from "../../util/genID";
import {anchorAt} from "./anchors";
import {annotationField, changeAnnotation} from "./extension";
import type {MarkdownAnnotation} from "./types";

export interface AnnotationLabels {
    title: string;
    add: string;
    orphan: string;
    reattach: string;
    select: string;
    all: string;
    margin: string;
    edit: string;
    remove: string;
    save: string;
    cancel: string;
    close: string;
}

const sourceSelection = (view: EditorView) => {
    const range = view.state.selection.main;
    const selection = view.dom.ownerDocument.getSelection();
    const inWidget = (node: Node | null) => {
        const element = node?.nodeType === Node.ELEMENT_NODE ? node as Element : node?.parentElement;
        return element && view.dom.contains(element) && element.closest('[contenteditable="false"]');
    };
    return range.empty || selection && !selection.isCollapsed && (inWidget(selection.anchorNode) || inWidget(selection.focusNode))
        ? undefined : range;
};

export const annotationQuoteText = (state: EditorState, record: MarkdownAnnotation): string => {
    if (record.status !== "attached" || record.from < 0 || record.from >= record.to || record.to > state.doc.length) {
        return record.quote;
    }
    return projectMarkdownRange(state, record.from, record.to).text;
};

export const stackAnnotationCards = (items: {id: string; top: number; height: number}[]) => {
    let bottom = 0;
    return items.map((item) => {
        const top = Math.max(0, item.top, bottom);
        bottom = top + item.height + 8;
        return {...item, top};
    });
};

export class MarkdownAnnotationSidebar {
    private readonly panel: HTMLElement;
    private readonly cards: HTMLElement;
    private readonly count: HTMLElement;
    private readonly hint: HTMLElement;
    private readonly modeButton: HTMLButtonElement;
    private readonly elements = new Map<string, HTMLElement>();
    private readonly observer: ResizeObserver;
    private active: string;
    private editing?: {id: string; note: string; fresh: boolean};
    private opened = false;
    private list = false;
    private disposed = false;
    private revealActive = false;
    private guidance = "";

    constructor(private view: EditorView, private root: HTMLElement, private scroll: HTMLElement,
                private labels: AnnotationLabels, private sourceMode: () => void) {
        this.panel = document.createElement("aside");
        this.panel.className = "markdown-annotations";
        this.panel.setAttribute("aria-label", labels.title);
        const header = document.createElement("div");
        header.className = "markdown-annotations__header";
        this.count = document.createElement("strong");
        this.modeButton = this.button(labels.all, () => {
            this.list = !this.list;
            this.modeButton.textContent = this.list ? labels.margin : labels.all;
            this.schedule();
        });
        header.append(this.count, this.modeButton, this.button(labels.close, () => this.toggle(false)));
        this.hint = document.createElement("div");
        this.hint.className = "markdown-annotations__hint";
        this.cards = document.createElement("div");
        this.cards.className = "markdown-annotations__cards";
        this.panel.append(header, this.button(labels.add, () => this.add()), this.hint, this.cards);
        root.append(this.panel);
        this.observer = new ResizeObserver(() => this.schedule());
        this.observer.observe(root);
        scroll.addEventListener("scroll", this.schedule, {passive: true});
        view.dom.addEventListener("click", this.onMarkClick);
        this.panel.addEventListener("keydown", this.onKeydown);
        this.toggle(view.state.field(annotationField).length > 0);
    }

    private button(label: string, run: () => void) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "b3-button b3-button--cancel";
        button.textContent = label;
        button.addEventListener("click", run);
        return button;
    }

    private readonly onMarkClick = (event: MouseEvent) => {
        const target = (event.target as HTMLElement).closest<HTMLElement>("[data-annotation-id]");
        if (target) this.select(target.dataset.annotationId, false);
    };

    private readonly onKeydown = (event: KeyboardEvent) => {
        if (event.isComposing) return;
        if (event.key === "Escape") {
            if (this.editing) this.finishEdit(true);
            else this.toggle(false);
            event.preventDefault();
            event.stopPropagation();
        } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && this.editing) {
            this.finishEdit(false);
            event.preventDefault();
        }
    };

    toggle(open = !this.opened) {
        this.opened = open;
        this.panel.hidden = !open;
        this.root.classList.toggle("markdown-editor--annotations", open);
        if (!open) this.view.focus();
        this.schedule();
    }

    add() {
        if (this.view.state.readOnly) return false;
        const range = sourceSelection(this.view);
        this.toggle(true);
        if (!range) {
            this.guidance = this.labels.select;
            this.hint.textContent = this.guidance;
            this.sourceMode();
            return false;
        }
        if (this.editing) this.finishEdit(false);
        const record: MarkdownAnnotation = {...anchorAt(this.view.state.doc.toString(), range.from, range.to),
            id: genUUID(), note: "", createdAt: Date.now(), updatedAt: Date.now()};
        this.active = record.id;
        this.revealActive = true;
        this.guidance = "";
        this.editing = {id: record.id, note: "", fresh: true};
        this.change(record);
        this.schedule();
        return true;
    }

    private change(value: MarkdownAnnotation | string) {
        if (this.view.state.readOnly) return;
        this.view.dispatch({effects: changeAnnotation.of(value), annotations: Transaction.addToHistory.of(false)});
    }

    private finishEdit(cancel: boolean) {
        const editing = this.editing;
        if (!editing) return;
        this.editing = undefined;
        const record = this.view.state.field(annotationField).find((item) => item.id === editing.id);
        if (cancel && record) this.change(editing.fresh ? editing.id : {...record, note: editing.note, updatedAt: Date.now()});
        this.elements.get(editing.id)?.remove();
        this.elements.delete(editing.id);
        this.view.focus();
        this.schedule();
    }

    select(id: string, navigate = true) {
        this.active = id;
        this.revealActive = true;
        this.toggle(true);
        const record = this.view.state.field(annotationField).find((item) => item.id === id);
        if (navigate && record?.status === "attached") {
            this.view.dispatch({selection: {anchor: record.from, head: record.to},
                effects: EditorView.scrollIntoView(record.from, {y: "center"})});
            this.view.focus();
        }
        this.schedule();
    }

    update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged || update.geometryChanged || syntaxTree(update.startState) !== syntaxTree(update.state) || update.startState.readOnly !== update.state.readOnly ||
            update.startState.field(annotationField) !== update.state.field(annotationField)) this.schedule();
    }

    private renderCard(record: MarkdownAnnotation) {
        const card = document.createElement("article");
        card.className = "markdown-annotation-card";
        card.dataset.annotationId = record.id;
        const quote = this.button(annotationQuoteText(this.view.state, record), () => this.select(record.id));
        quote.className = "markdown-annotation-card__quote";
        const body = document.createElement("div");
        body.className = "markdown-annotation-card__note";
        card.append(quote, body);
        if (this.editing?.id === record.id) {
            const input = document.createElement("textarea");
            input.className = "b3-text-field fn__block";
            input.setAttribute("aria-label", this.labels.title);
            input.maxLength = 65536;
            input.value = record.note;
            input.addEventListener("input", () => {
                const current = this.view.state.field(annotationField).find((item) => item.id === record.id);
                if (current) this.change({...current, note: input.value, updatedAt: Date.now()});
            });
            body.append(input, this.button(this.labels.save, () => this.finishEdit(false)),
                this.button(this.labels.cancel, () => this.finishEdit(true)));
            requestAnimationFrame(() => { if (!this.disposed && input.isConnected) input.focus(); });
        } else {
            body.textContent = record.note;
            if (!this.view.state.readOnly) {
                card.append(this.button(this.labels.edit, () => {
                    if (this.editing) this.finishEdit(false);
                    this.editing = {id: record.id, note: record.note, fresh: false};
                    this.active = record.id;
                    this.elements.delete(record.id);
                    card.remove();
                    this.schedule();
                }), this.button(this.labels.remove, () => this.change(record.id)));
            }
        }
        const status = document.createElement("div");
        status.className = "markdown-annotation-card__status";
        if (record.status === "orphaned") {
            status.textContent = this.labels.orphan;
            if (!this.view.state.readOnly) status.append(this.button(this.labels.reattach, () => {
                const range = sourceSelection(this.view);
                if (!range) { this.guidance = this.labels.select; this.hint.textContent = this.guidance; this.sourceMode(); return; }
                this.guidance = "";
                this.change({...record, ...anchorAt(this.view.state.doc.toString(), range.from, range.to), updatedAt: Date.now()});
            }));
        }
        card.append(status);
        this.observer.observe(card);
        return card;
    }

    readonly schedule = () => {
        if (this.disposed || !this.opened) return;
        this.view.requestMeasure({key: this, read: () => {
            const records = this.view.state.field(annotationField);
            const narrow = this.root.clientWidth < 900;
            const list = this.list || narrow;
            const top = this.cards.getBoundingClientRect().top;
            const height = this.scroll.clientHeight;
            const items = records.slice().sort((a, b) => a.from - b.from).flatMap((record) => {
                let y = 0;
                if (!list && record.status === "attached") {
                    const coords = this.view.coordsAtPos(Math.min(this.view.state.doc.length, record.from));
                    if (!coords && this.active !== record.id) return [];
                    y = (coords?.top ?? top) - top;
                    if ((y < -200 || y > height + 200) && this.active !== record.id) return [];
                } else if (!list && this.active !== record.id) return [];
                return [{id: record.id, record, top: list ? 0 : y,
                    height: this.elements.get(record.id)?.getBoundingClientRect().height || 100}];
            });
            return {items: stackAnnotationCards(items), records, narrow, list,
                panelHeight: this.cards.clientHeight, panelScroll: this.cards.scrollTop};
        }, write: ({items, records, narrow, list, panelHeight, panelScroll}) => {
            if (this.disposed) return;
            this.root.classList.toggle("markdown-editor--annotations-narrow", narrow);
            this.panel.classList.toggle("markdown-annotations--list", list);
            this.modeButton.hidden = narrow;
            this.count.textContent = `${this.labels.title} (${records.length})`;
            const orphanCount = records.filter((item) => item.status === "orphaned").length;
            this.hint.textContent = this.guidance || (orphanCount ? `${this.labels.orphan} (${orphanCount})` : "");
            const visible = new Set(items.map((item) => item.id));
            for (const [id, card] of this.elements) {
                if (!visible.has(id) && this.editing?.id !== id) {
                    this.observer.unobserve(card); card.remove(); this.elements.delete(id);
                }
            }
            for (const item of items) {
                const record = records.find((candidate) => candidate.id === item.id);
                let card = this.elements.get(item.id);
                const signature = `${this.view.state.readOnly}:${record.status}:${record.quote}:${record.note}`;
                if (card && this.editing?.id !== item.id && card.dataset.signature !== signature) {
                    this.observer.unobserve(card); card.remove(); this.elements.delete(item.id); card = undefined;
                }
                if (!card) {
                    card = this.renderCard(record);
                    this.elements.set(item.id, card);
                    this.cards.append(card);
                }
                card.dataset.signature = signature;
                const quote = card.querySelector<HTMLElement>(".markdown-annotation-card__quote");
                const displayQuote = annotationQuoteText(this.view.state, record);
                if (quote && quote.textContent !== displayQuote) quote.textContent = displayQuote;
                card.classList.toggle("markdown-annotation-card--active", item.id === this.active);
                card.style.top = list ? "" : `${item.top}px`;
                const input = card.querySelector("textarea");
                if (input) input.readOnly = this.view.state.readOnly;
            }
            if (list && !this.editing) {
                for (const item of items) this.cards.append(this.elements.get(item.id));
            }
            if (this.revealActive && !list) {
                const item = items.find((candidate) => candidate.id === this.active);
                if (item && (item.top < panelScroll || item.top + item.height > panelScroll + panelHeight)) {
                    this.cards.scrollTop = Math.max(0, item.top - (panelHeight - item.height) / 2);
                }
                this.revealActive = false;
            }
        }});
    };

    destroy() {
        this.disposed = true;
        this.observer.disconnect();
        this.scroll.removeEventListener("scroll", this.schedule);
        this.view.dom.removeEventListener("click", this.onMarkClick);
        this.panel.removeEventListener("keydown", this.onKeydown);
        this.panel.remove();
        this.root.classList.remove("markdown-editor--annotations", "markdown-editor--annotations-narrow");
    }
}
