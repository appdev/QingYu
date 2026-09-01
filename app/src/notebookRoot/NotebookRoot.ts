import {Model} from "../layout/Model";
import type {App} from "../index";
import type {Tab} from "../layout/Tab";
import {fetchPost} from "../util/fetch";
import {escapeAttr, escapeHtml} from "../util/escape";
import {unicode2Emoji} from "../emoji";
import {openFileById, openMarkdownFile} from "../editor/util";
import {openNewFileMenu} from "../markdown/fileActions";
import {replaceFileName, validateName} from "../editor/rename";
import {notebookRootView, setNotebookRootView} from "./viewState";
import {renderNotebookRootDocuments} from "./render";
import type {NotebookRootListing, NotebookRootView} from "./types";
import {DocumentCardPreviewController} from "./previewController";
import {NotebookRootDragController} from "./drag";
import {sortMenu} from "../menus/navigation";
import {goBack} from "../util/backForward";
import {Constants} from "../constants";
import {setNotebookName} from "../util/pathName";
/// #if !MOBILE
import {openSearch} from "../search/spread";
/// #else
import {popSearch} from "../mobile/menu/search";
/// #endif

export class NotebookRoot extends Model {
    public readonly notebookId: string;
    public readonly element: HTMLElement;
    public listing: NotebookRootListing;
    private view: NotebookRootView;
    private destroyed = false;
    private previewController: DocumentCardPreviewController;
    private dragController: NotebookRootDragController;
    private selectedDocumentPath?: string;
    private themeMode: string;
    private readonly themeObserver: MutationObserver;

    constructor(options: {app: App, tab?: Tab, element: HTMLElement, notebookId: string, name: string}) {
        super({app: options.app});
        this.notebookId = options.notebookId;
        this.element = options.element;
        this.view = notebookRootView(this.notebookId);
        this.listing = {notebook: this.notebookId, name: options.name, icon: "", sortMode: 0, documents: []};
        this.themeMode = document.documentElement.dataset.themeMode || "";
        this.themeObserver = new MutationObserver(() => {
            const themeMode = document.documentElement.dataset.themeMode || "";
            if (this.destroyed || themeMode === this.themeMode) {
                return;
            }
            this.themeMode = themeMode;
            const scrollTop = this.element.querySelector<HTMLElement>(".notebook-root")?.scrollTop || 0;
            this.renderShell();
            requestAnimationFrame(() => {
                const root = this.element.querySelector<HTMLElement>(".notebook-root");
                if (root) {
                    root.scrollTop = scrollTop;
                }
            });
        });
        this.themeObserver.observe(document.documentElement, {attributes: true, attributeFilter: ["data-theme-mode"]});
        this.renderShell();
        void this.reload();
    }

    public reload() {
        return new Promise<void>((resolve) => {
            fetchPost("/api/notebook/listRootDocuments", {notebook: this.notebookId}, (response) => {
                if (!this.destroyed && response.code === 0) {
                    this.listing = response.data as NotebookRootListing;
                    this.renderShell();
                    this.parent?.updateTitle(this.listing.name);
                }
                resolve();
            });
        });
    }

    public applyNotebookName(name: string) {
        this.listing.name = name;
        this.renderShell();
        this.parent?.updateTitle(name);
    }

    public handleEvent(data: IWebSocketData) {
        const eventData = data.data as {box?: string, oldBox?: string};
        if (eventData?.box === this.notebookId || eventData?.oldBox === this.notebookId) {
            void this.reload();
        }
    }

    public destroy() {
        this.destroyed = true;
        this.themeObserver.disconnect();
        this.previewController?.destroy();
        this.dragController?.destroy();
        this.element.replaceChildren();
    }

    private renderShell() {
        this.previewController?.destroy();
        this.dragController?.destroy();
        const icon = this.listing.icon ? unicode2Emoji(this.listing.icon) : "";
        this.element.innerHTML = `<div class="notebook-root" data-notebook="${escapeAttr(this.notebookId)}" data-view="${this.view}">
    <header class="notebook-root__toolbar">
        <div class="notebook-root__toolbar-group notebook-root__toolbar-group--leading">
            <button class="notebook-root__action b3-tooltips__n" data-action="back" aria-label="${escapeAttr(window.siyuan.languages.goBack)}"><svg aria-hidden="true"><use xlink:href="#iconBack"></use></svg></button>
            <button class="notebook-root__action notebook-root__new b3-tooltips__n" data-action="new" data-menu="true" aria-label="${escapeAttr(window.siyuan.languages.newFile)}"${window.siyuan.config.readonly ? " disabled" : ""}><svg aria-hidden="true"><use xlink:href="#iconAdd"></use></svg></button>
            <div class="notebook-root__title"><span>${icon}</span><span class="notebook-root__title-editable" data-action="rename" contenteditable="${window.siyuan.config.readonly ? "false" : "plaintext-only"}" role="textbox" aria-label="${escapeAttr(window.siyuan.languages.rename)}" spellcheck="false">${escapeHtml(this.listing.name)}</span></div>
        </div>
        <div class="fn__flex-1"></div>
        <div class="notebook-root__toolbar-group notebook-root__toolbar-group--trailing">
            <div class="notebook-root__views">
                ${this.viewButton("masonry", "iconLayout", window.siyuan.languages.notebookRootMasonry || "瀑布流")}
                ${this.viewButton("large", "iconGallery", window.siyuan.languages.notebookRootLargePreview || "大预览")}
                ${this.viewButton("list", "iconList", window.siyuan.languages.notebookRootList || "列表")}
            </div>
            <button class="notebook-root__action b3-tooltips__n" data-action="sort" data-menu="true" aria-label="${escapeAttr(window.siyuan.languages.sort)}"><svg aria-hidden="true"><use xlink:href="#iconSort"></use></svg></button>
            <button class="notebook-root__action b3-tooltips__n" data-action="search" aria-label="${escapeAttr(window.siyuan.languages.search)}"><svg aria-hidden="true"><use xlink:href="#iconSearch"></use></svg></button>
        </div>
    </header>
    <main class="notebook-root__documents notebook-root__documents--${this.view}">${renderNotebookRootDocuments(this.listing.documents, this.view, this.listing)}</main>
</div>`;
        if (this.selectedDocumentPath) {
            const selected = Array.from(this.element.querySelectorAll<HTMLElement>(".notebook-root__document"))
                .find((document) => document.dataset.path === this.selectedDocumentPath);
            if (selected) {
                selected.classList.add("notebook-root__document--selected");
            } else {
                this.selectedDocumentPath = undefined;
            }
        }
        this.bindEvents();
        this.previewController = new DocumentCardPreviewController();
        this.element.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((document) => {
            this.previewController.observe(document, {
                kind: document.dataset.kind as "sy" | "markdown",
                notebook: document.dataset.notebook,
                path: document.dataset.path,
                id: document.dataset.id,
                identityState: document.dataset.identityState,
                identityConflict: document.dataset.identityConflict === "true",
                revision: document.dataset.revision,
                updated: Number(document.dataset.updated) || 0,
                sourceSize: Number(document.dataset.sourceSize) || 0,
            });
        });
        this.dragController = new NotebookRootDragController({
            element: this.element,
            notebook: this.notebookId,
            sortMode: this.listing.sortMode,
            documents: this.listing.documents,
            reload: () => this.reload(),
        });
    }

    private viewButton(view: NotebookRootView, icon: string, label: string) {
        return `<button class="b3-tooltips__n${view === this.view ? " notebook-root__view--active" : ""}" data-view="${view}" aria-label="${escapeAttr(label)}" aria-pressed="${view === this.view}"><svg aria-hidden="true"><use xlink:href="#${icon}"></use></svg></button>`;
    }

    private bindEvents() {
        this.element.querySelector<HTMLElement>("[data-action='back']")?.addEventListener("click", () => {
            void goBack(this.app);
        });
        this.element.querySelector<HTMLElement>("[data-action='new']")?.addEventListener("click", (event) => {
            const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
            openNewFileMenu(this.app, {notebookId: this.notebookId, currentPath: "/", position: {x: rect.left, y: rect.bottom}});
        });
        const titleElement = this.element.querySelector<HTMLElement>("[data-action='rename']");
        let titleEditCancelled = false;
        const saveTitle = () => {
            if (!titleElement || titleEditCancelled) {
                titleEditCancelled = false;
                if (titleElement) titleElement.textContent = this.listing.name;
                return;
            }
            if (!validateName(titleElement.textContent, titleElement)) {
                titleElement.textContent = this.listing.name;
                return;
            }
            let name = replaceFileName(titleElement.textContent.trim());
            if (!name) name = window.siyuan.languages.untitled;
            if (name === this.listing.name) {
                titleElement.textContent = this.listing.name;
                return;
            }
            fetchPost("/api/notebook/renameNotebook", {notebook: this.notebookId, name}, (response) => {
                if (response.code !== 0) {
                    titleElement.textContent = this.listing.name;
                    return;
                }
                setNotebookName(this.notebookId, name);
                this.applyNotebookName(name);
            });
        };
        titleElement?.addEventListener("keydown", (event) => {
            if (event.isComposing) return;
            if (event.key === "Enter") {
                event.preventDefault();
                titleElement.blur();
            } else if (event.key === "Escape") {
                event.preventDefault();
                titleEditCancelled = true;
                titleElement.textContent = this.listing.name;
                titleElement.blur();
            }
        });
        titleElement?.addEventListener("blur", saveTitle);
        this.element.querySelector<HTMLElement>("[data-action='sort']")?.addEventListener("click", (event) => {
            window.siyuan.menus.menu.remove();
            sortMenu("notebook", this.listing.sortMode, (sortMode) => {
                fetchPost("/api/notebook/setNotebookConf", {notebook: this.notebookId, conf: {sortMode}}, () => void this.reload());
            }).forEach((item) => window.siyuan.menus.menu.addItem(item));
            const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
            window.siyuan.menus.menu.popup({x: rect.left, y: rect.bottom});
        });
        this.element.querySelector<HTMLElement>("[data-action='search']")?.addEventListener("click", () => {
            /// #if MOBILE
            popSearch(this.app, {
                hasReplace: false,
                hPath: this.listing.name,
                idPath: [this.notebookId],
                page: 1,
            });
            /// #else
            void openSearch({
                app: this.app,
                hotkey: Constants.DIALOG_SEARCH,
                notebookId: this.notebookId,
            });
            /// #endif
        });
        this.element.querySelectorAll<HTMLElement>(".notebook-root__views [data-view]").forEach((button) => button.addEventListener("click", () => {
            const view = button.dataset.view as NotebookRootView;
            if (!view || view === this.view) return;
            this.view = view;
            setNotebookRootView(this.notebookId, view);
            this.renderShell();
        }));
        this.element.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((document) => {
            document.addEventListener("pointerdown", (event) => {
                if (event.button !== 0) return;
                this.selectedDocumentPath = document.dataset.path;
                this.element.querySelectorAll(".notebook-root__document--selected").forEach((item) => {
                    item.classList.remove("notebook-root__document--selected");
                });
                document.classList.add("notebook-root__document--selected");
            });
            const open = () => {
                if (document.dataset.kind === "markdown") {
                    void openMarkdownFile(this.app, this.notebookId, document.dataset.path, document.querySelector(".notebook-root__document-title")?.textContent || "Markdown");
                } else {
                    void openFileById({app: this.app, id: document.dataset.id});
                }
            };
            document.addEventListener("dblclick", open);
            document.addEventListener("keydown", (event) => {
                if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    open();
                }
            });
        });
    }
}
