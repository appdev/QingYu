import {type EditorView} from "@codemirror/view";
import {captureContext, type ActionHost, type FormatAction, type SelectionContext} from "./context";
import {executeAction, readActionState} from "./commands";
import {actionPresentation} from "./menu";

const formatActions: FormatAction[] = ["link", "bold", "italic", "strike", "highlight", "code", "math", "clear", "annotation"];

export class SelectionToolbar {
    readonly element: HTMLElement;
    private context: SelectionContext;
    private dismissed = "";
    private disposed = false;
    private readonly selectionChange = () => this.refresh();
    private readonly focusChange = () => this.refresh();
    private readonly scroll = () => this.position();

    constructor(private view: EditorView, private host: ActionHost) {
        this.element = document.createElement("div");
        this.element.className = "markdown-selection-toolbar";
        this.element.role = "toolbar";
        this.element.hidden = true;
        view.dom.ownerDocument.body.append(this.element);
        this.element.addEventListener("mousedown", (event) => event.preventDefault());
        this.element.addEventListener("keydown", (event) => {
            if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                this.hide();
                view.focus();
            }
            if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
                const buttons = Array.from(this.element.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
                const index = buttons.indexOf(event.target as HTMLButtonElement);
                buttons[(index + (event.key === "ArrowRight" ? 1 : buttons.length - 1)) % buttons.length]?.focus();
                event.preventDefault();
            }
        });
        view.dom.ownerDocument.addEventListener("selectionchange", this.selectionChange);
        view.dom.ownerDocument.addEventListener("focusin", this.focusChange);
        view.dom.ownerDocument.addEventListener("scroll", this.scroll, true);
        window.addEventListener("resize", this.scroll);
    }

    private signature() {
        const range = this.view.state.selection.main;
        return `${this.host.mode()}:${range.anchor}:${range.head}:${this.view.state.doc.length}`;
    }

    hide() {
        this.dismissed = this.signature();
        this.element.hidden = true;
    }

    refresh() {
        if (this.disposed) return;
        const active = this.view.dom.ownerDocument.activeElement;
        if (this.element.contains(active)) { this.position(); return; }
        const signature = this.signature();
        if (!this.view.hasFocus || this.view.composing || this.view.state.readOnly || this.dismissed === signature) {
            this.element.hidden = true;
            return;
        }
        const selection = this.view.dom.ownerDocument.getSelection();
        const node = selection?.anchorNode;
        const origin = node instanceof Element ? node : node?.parentElement;
        const context = captureContext(this.view, origin && this.view.contentDOM.contains(origin) ? origin : this.view.contentDOM, this.host.mode());
        if (!context || context.kind === "table-cell") { this.element.hidden = true; return; }
        const actions = this.host.mode() === "source" || context.kind === "preview" ? ["annotation" as const] : formatActions;
        if (!actions.some((id) => readActionState(id, context).enabled)) { this.element.hidden = true; return; }
        this.context = context;
        this.element.replaceChildren();
        for (const id of actions) {
            const {label, icon} = actionPresentation(id);
            const state = readActionState(id, context);
            const button = document.createElement("button");
            button.type = "button";
            button.className = "block__icon block__icon--show";
            button.dataset.action = id;
            button.setAttribute("aria-label", label);
            button.title = label;
            button.disabled = !state.enabled;
            if (["bold", "italic", "strike", "highlight", "code", "math"].includes(id)) button.setAttribute("aria-pressed", String(state.active));
            button.innerHTML = `<svg><use xlink:href="#${icon}"></use></svg>`;
            button.addEventListener("click", () => {
                const current = this.context;
                this.hide();
                void executeAction(id, current, this.host);
            });
            this.element.append(button);
        }
        this.element.hidden = false;
        this.position();
    }

    position() {
        if (this.disposed || this.element.hidden) return;
        this.view.requestMeasure({key: this, read: () => {
            const rect = this.context?.kind === "preview" ? this.context.domRange?.getBoundingClientRect() :
                this.view.coordsAtPos(this.view.state.selection.main.head);
            return {rect, width: this.element.offsetWidth, height: this.element.offsetHeight};
        }, write: ({rect, width, height}) => {
            if (this.disposed || !rect) return;
            this.element.style.left = `${Math.max(8, Math.min(window.innerWidth - width - 8, rect.left))}px`;
            this.element.style.top = `${Math.max(8, Math.min(window.innerHeight - height - 8, rect.top > height + 8 ? rect.top - height - 8 : rect.bottom + 8))}px`;
        }});
    }

    focus() {
        this.dismissed = "";
        this.refresh();
        this.element.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    }

    destroy() {
        this.disposed = true;
        this.view.dom.ownerDocument.removeEventListener("selectionchange", this.selectionChange);
        this.view.dom.ownerDocument.removeEventListener("focusin", this.focusChange);
        this.view.dom.ownerDocument.removeEventListener("scroll", this.scroll, true);
        window.removeEventListener("resize", this.scroll);
        this.element.remove();
    }
}
