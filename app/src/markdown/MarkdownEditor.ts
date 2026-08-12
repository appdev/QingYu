import {EditorView, minimalSetup} from "codemirror";
import {Compartment} from "@codemirror/state";
import {Model} from "../layout/Model";
import {Tab} from "../layout/Tab";
import {App} from "../index";
import {fetchSyncPost} from "../util/fetch";
import {confirmDialog} from "../dialog/confirmDialog";
import {escapeHtml} from "../util/escape";
import {saveLayout, setPanelFocus} from "../layout/util";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {createSiyuanMarkdownAdapter} from "./siyuanAdapter";
import {trackMarkdownFlush} from "./saveBarrier";
import {acquireMarkdownThemeBridge} from "./markdownThemeBridge";
import {isMarkdownSelectAll, selectElementContents} from "./keyboard";

interface IMarkdownDocument {
    path: string;
    name: string;
    content: string;
    revision: string;
    mtime: number;
}

export class MarkdownEditor extends Model {
    public element: HTMLElement;
    public headElement: HTMLElement;
    public notebookId: string;
    public path: string;
    public view: EditorView;
    private revision: string;
    private saveTimer: number;
    private saving = false;
    private dirty = false;
    private lastSaveStatus: "saved" | "conflict" | "error" = "saved";
    private savePromise: Promise<boolean>;
    private preview = true;
    private destroyed = false;
    private modeCompartment = new Compartment();
    private titleElement: HTMLElement;
    private surfaceElement: HTMLElement;
    private statusElement: HTMLElement;
    private releaseThemeBridge: () => void;

    constructor(options: {app: App, tab?: Tab, element?: HTMLElement, notebookId: string, path: string}) {
        super({app: options.app});
        this.element = options.tab?.panelElement || options.element;
        this.headElement = options.tab?.headElement;
        this.notebookId = options.notebookId;
        this.path = options.path;
        if (!this.element) {
            throw new Error("Markdown editor element is required");
        }
        if (this.headElement && window.siyuan.config.fileTree.openFilesUseCurrentTab) {
            this.headElement.classList.add("item--unupdate");
        }
        this.renderShell();
        this.releaseThemeBridge = acquireMarkdownThemeBridge(this.element.ownerDocument);
        this.load();
    }

    public save() {
        return this.flush();
    }

    public flush() {
        window.clearTimeout(this.saveTimer);
        if (!this.savePromise) {
            this.savePromise = this.flushDirty().finally(() => {
                this.savePromise = undefined;
            });
        }
        return trackMarkdownFlush(this.savePromise);
    }

    private async flushDirty() {
        while (this.dirty) {
            if (!await this.saveOnce()) {
                return false;
            }
        }
        return this.lastSaveStatus === "saved";
    }

    private async saveOnce() {
        const view = this.view;
        if (!this.dirty) {
            return true;
        }
        if (!view) {
            return false;
        }
        this.saving = true;
        this.setStatus("saving");
        const content = view.state.doc.toString();
        let response: IWebSocketData;
        try {
            response = await fetchSyncPost("/api/markdown/save", {
                notebook: this.notebookId,
                path: this.path,
                content,
                revision: this.revision,
            });
        } catch {
            this.saving = false;
            this.lastSaveStatus = "error";
            if (!this.destroyed) this.setStatus("error");
            return false;
        }
        this.saving = false;
        if (response.code === 0) {
            const document = response.data as IMarkdownDocument;
            this.revision = document.revision;
            this.dirty = view.state.doc.toString() !== content;
            this.lastSaveStatus = "saved";
            if (!this.destroyed) {
                this.setStatus(this.dirty ? "dirty" : "saved");
            }
            return true;
        } else if (response.code === 409) {
            this.lastSaveStatus = "conflict";
            if (!this.destroyed) {
                this.setStatus("conflict");
                confirmDialog(window.siyuan.languages.conflict, window.siyuan.languages.refresh, () => {
                    this.dirty = false;
                    this.view.destroy();
                    this.surfaceElement.innerHTML = "";
                    void this.load();
                });
            }
        } else {
            this.lastSaveStatus = "error";
            if (!this.destroyed) this.setStatus("error");
        }
        return false;
    }

    public async rename(name: string) {
        const response = await fetchSyncPost("/api/markdown/rename", {
            notebook: this.notebookId,
            path: this.path,
            name,
        });
        if (response.code !== 0) {
            this.titleElement.textContent = this.fileName();
            return false;
        }
        const document = response.data as IMarkdownDocument;
        this.path = document.path;
        this.revision = document.revision;
        this.titleElement.textContent = document.name;
        this.updateTitle(document.name);
        this.renderBreadcrumb();
        saveLayout();
        return true;
    }

    public destroy() {
        window.clearTimeout(this.saveTimer);
        if (this.dirty) {
            void this.flush();
        }
        this.destroyed = true;
        this.view?.destroy();
        this.releaseThemeBridge?.();
        if (process.env.NODE_ENV === "development") {
            delete (this.element as HTMLElement & {__markdownEditorView?: EditorView}).__markdownEditorView;
        }
    }

    private renderShell() {
        this.element.innerHTML = `<div class="protyle-breadcrumb markdown-editor__breadcrumb">
    <div class="protyle-breadcrumb__bar"></div>
    <span class="protyle-breadcrumb__space"></span>
    <button class="block__icon fn__flex-center ariaLabel" data-type="markdown-source" aria-label="Markdown"><svg><use xlink:href="#iconEdit"></use></svg></button>
    <button class="block__icon block__icon--active fn__flex-center ariaLabel" data-type="markdown-preview" aria-label="${escapeHtml(window.siyuan.languages.wysiwyg)}"><svg><use xlink:href="#iconPreview"></use></svg></button>
    <span class="markdown-editor__status" aria-live="polite"></span>
</div>
<div class="protyle-content markdown-editor__content">
    <div class="protyle-top markdown-editor__top">
        <div class="protyle-title markdown-editor__title">
            <span class="protyle-title__icon"><svg><use xlink:href="#iconMarkdown"></use></svg></span>
            <div contenteditable="true" spellcheck="false" class="protyle-title__input"></div>
        </div>
    </div>
    <div class="markdown-editor__body">
        <div class="markdown-editor__surface b3-typography"></div>
    </div>
</div>`;
        this.element.classList.add("protyle", "markdown-editor");
        this.element.classList.toggle("markdown-editor--full-width", window.siyuan.config.editor.fullWidth);
        this.titleElement = this.element.querySelector(".protyle-title__input");
        this.surfaceElement = this.element.querySelector(".markdown-editor__surface");
        this.statusElement = this.element.querySelector(".markdown-editor__status");
        this.element.addEventListener("click", (event) => {
            if (this.element.parentElement?.parentElement) {
                setPanelFocus(this.element.parentElement.parentElement);
            }
            const target = event.target as HTMLElement;
            const button = target.closest("button");
            if (button?.dataset.type === "markdown-source") {
                this.setPreview(false);
            } else if (button?.dataset.type === "markdown-preview") {
                this.setPreview(true);
            }
        });
        this.element.addEventListener("keydown", (event: KeyboardEvent) => {
            if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
                event.preventDefault();
                event.stopPropagation();
                void this.save();
            }
        });
        this.titleElement.addEventListener("keydown", (event: KeyboardEvent) => {
            if (isMarkdownSelectAll(event)) {
                event.preventDefault();
                event.stopPropagation();
                selectElementContents(this.titleElement);
            } else if (event.key === "Enter") {
                event.preventDefault();
                this.titleElement.blur();
            }
        });
        this.titleElement.addEventListener("blur", () => {
            const name = this.titleElement.textContent.trim();
            if (name && name !== this.fileName()) {
                void this.rename(name);
            } else {
                this.titleElement.textContent = this.fileName();
            }
        });
    }

    private async load() {
        const response = await fetchSyncPost("/api/markdown/get", {
            notebook: this.notebookId,
            path: this.path,
        });
        if (response.code !== 0) {
            this.setStatus("error");
            return;
        }
        const document = response.data as IMarkdownDocument;
        if (this.destroyed) {
            return;
        }
        this.path = document.path;
        this.revision = document.revision;
        this.titleElement.textContent = document.name;
        this.updateTitle(document.name);
        this.renderBreadcrumb();
        const adapter = createSiyuanMarkdownAdapter({
            app: this.app,
            documentPath: () => this.path,
        });
        this.view = new EditorView({
            doc: document.content,
            extensions: [
                minimalSetup,
                this.modeCompartment.of(createSiyuanMarkraExtension({
                    adapter,
                    documentPath: () => this.path,
                    mode: "visual",
                })),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        this.dirty = true;
                        this.setStatus("dirty");
                        this.scheduleSave();
                    }
                }),
            ],
            parent: this.surfaceElement,
        });
        if (process.env.NODE_ENV === "development") {
            Object.defineProperty(this.element, "__markdownEditorView", {
                configurable: true,
                value: this.view,
            });
        }
        this.setStatus("saved");
        this.setPreview(true, false);
    }

    private scheduleSave() {
        window.clearTimeout(this.saveTimer);
        this.saveTimer = window.setTimeout(() => {
            void this.save();
        }, 800);
    }

    private updateTitle(name: string) {
        this.parent?.updateTitle(name);
        const toolbarNameElement = document.getElementById("toolbarName") as HTMLInputElement;
        if (!this.headElement && toolbarNameElement) {
            toolbarNameElement.value = name;
        }
    }

    private setPreview(preview: boolean, focus = true) {
        this.preview = preview;
        this.element.querySelector('[data-type="markdown-source"]')?.classList.toggle("block__icon--active", !preview);
        this.element.querySelector('[data-type="markdown-preview"]')?.classList.toggle("block__icon--active", preview);
        if (this.view) {
            this.view.dispatch({
                effects: this.modeCompartment.reconfigure(createSiyuanMarkraExtension({
                    adapter: createSiyuanMarkdownAdapter({
                        app: this.app,
                        documentPath: () => this.path,
                    }),
                    documentPath: () => this.path,
                    mode: preview ? "visual" : "source",
                })),
            });
            if (focus) {
                this.view.focus();
            } else {
                this.view.dispatch({selection: {anchor: this.view.state.doc.length}});
                this.view.scrollDOM.scrollTop = 0;
            }
        }
    }

    private renderBreadcrumb() {
        const notebook = window.siyuan.notebooks.find((item) => item.id === this.notebookId);
        const parts = [notebook?.name || this.notebookId, ...this.path.split("/").filter(Boolean)];
        this.element.querySelector(".protyle-breadcrumb__bar").innerHTML = parts.map((item, index) => `<span class="protyle-breadcrumb__item${index === parts.length - 1 ? " protyle-breadcrumb__item--active" : ""}">
    <span class="protyle-breadcrumb__text">${escapeHtml(item)}</span>
    ${index === parts.length - 1 ? "" : '<svg class="protyle-breadcrumb__arrow"><use xlink:href="#iconRight"></use></svg>'}
</span>`).join("");
    }

    private setStatus(status: "saved" | "saving" | "dirty" | "conflict" | "error") {
        this.statusElement.dataset.status = status;
        const icons = {
            saved: "iconCheck",
            saving: "iconRefresh",
            dirty: "iconEdit",
            conflict: "iconTriangleAlert",
            error: "iconClose",
        };
        this.statusElement.innerHTML = `<svg><use xlink:href="#${icons[status]}"></use></svg>`;
        this.statusElement.setAttribute("aria-label", status === "conflict" ? window.siyuan.languages.conflict : window.siyuan.languages.save);
    }

    private fileName() {
        return this.path.substring(this.path.lastIndexOf("/") + 1);
    }
}
