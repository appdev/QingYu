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
import type {NotebookRootDocument, NotebookRootListing, NotebookRootView} from "./types";
import {DocumentCardPreviewController} from "./previewController";
import {NotebookRootDragController} from "./drag";
import {updateNotebookRootTitleLayout} from "./titleLayout";
import {sortMenu} from "../menus/navigation";
import {Constants} from "../constants";
import {setNotebookName} from "../util/pathName";
import {isMobile} from "../util/functions";
import {notebookRootDocumentKey, notebookRootElementKey} from "./documentKey";
import {openNotebookRootContextMenu} from "./contextMenu";
import {NotebookRootMasonryController} from "./masonryController";
import {
    captureNotebookRootLayoutSnapshot,
    hydrateNotebookRootLayout,
    restoreNotebookRootScrollAnchor,
} from "./layoutSwitch";
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
    private masonryController?: NotebookRootMasonryController;
    private selectedDocumentKey?: string;
    private readonly themeObserver: MutationObserver;
    private themeRefreshFrame = 0;
    private readonly titleLayoutObserver: ResizeObserver;
    private titleLayoutFrame = 0;
    private readonly handleThemeApplied = () => this.scheduleThemeRefresh();
    private readonly openDocument?: (document: NotebookRootDocument) => void;

    constructor(options: {
        app: App,
        tab?: Tab,
        element: HTMLElement,
        notebookId: string,
        name: string,
        openDocument?: (document: NotebookRootDocument) => void,
    }) {
        super({app: options.app});
        this.notebookId = options.notebookId;
        this.element = options.element;
        this.view = notebookRootView(this.notebookId);
        this.openDocument = options.openDocument;
        this.listing = {notebook: this.notebookId, name: options.name, icon: "", sortMode: 0, documents: []};
        this.themeObserver = new MutationObserver((records) => {
            const standardThemeAttributes = ["data-theme-mode", "data-light-theme", "data-dark-theme"];
            if (records.some((record) => record.attributeName === "class" ||
                (record.attributeName?.includes("theme") && !standardThemeAttributes.includes(record.attributeName)))) {
                this.scheduleThemeRefresh();
            }
        });
        this.themeObserver.observe(document.documentElement, {attributes: true});
        this.titleLayoutObserver = new ResizeObserver(() => this.scheduleTitleLayout());
        this.titleLayoutObserver.observe(this.element);
        window.addEventListener("siyuan-theme-applied", this.handleThemeApplied);
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
        this.titleLayoutObserver.disconnect();
        window.removeEventListener("siyuan-theme-applied", this.handleThemeApplied);
        if (this.themeRefreshFrame) {
            cancelAnimationFrame(this.themeRefreshFrame);
        }
        if (this.titleLayoutFrame) {
            cancelAnimationFrame(this.titleLayoutFrame);
        }
        this.previewController?.destroy();
        this.dragController?.destroy();
        this.masonryController?.destroy();
        this.element.replaceChildren();
    }

    private renderShell() {
        this.previewController?.destroy();
        this.dragController?.destroy();
        this.masonryController?.destroy();
        this.masonryController = undefined;
        const icon = this.listing.icon ? unicode2Emoji(this.listing.icon) : "";
        this.element.innerHTML = `<div class="notebook-root" data-notebook="${escapeAttr(this.notebookId)}" data-view="${this.view}">
    <header class="notebook-root__toolbar">
        <div class="notebook-root__toolbar-group notebook-root__toolbar-group--leading">
            <button class="notebook-root__action block__icon block__icon--show b3-tooltips__n" data-action="new" data-menu="true" aria-label="${escapeAttr(window.siyuan.languages.newFile)}"${window.siyuan.config.readonly ? " disabled" : ""}><svg aria-hidden="true"><use xlink:href="#iconAdd"></use></svg></button>
            <div class="notebook-root__title"><span>${icon}</span><span class="notebook-root__title-editable" data-action="rename" contenteditable="${window.siyuan.config.readonly ? "false" : "plaintext-only"}" role="textbox" aria-label="${escapeAttr(window.siyuan.languages.rename)}" spellcheck="false">${escapeHtml(this.listing.name)}</span></div>
        </div>
        <div class="fn__flex-1"></div>
        <div class="notebook-root__toolbar-group notebook-root__toolbar-group--trailing">
            <div class="notebook-root__views">
                ${this.viewButton("masonry", "iconLayout", window.siyuan.languages.notebookRootMasonry || "瀑布流")}
                ${this.viewButton("large", "iconGallery", window.siyuan.languages.notebookRootLargePreview || "大预览")}
                ${this.viewButton("list", "iconList", window.siyuan.languages.notebookRootList || "列表")}
            </div>
            <button class="notebook-root__action block__icon block__icon--show b3-tooltips__n" data-action="sort" data-menu="true" aria-label="${escapeAttr(window.siyuan.languages.sort)}"><svg aria-hidden="true"><use xlink:href="#iconSort"></use></svg></button>
            <button class="notebook-root__action block__icon block__icon--show b3-tooltips__n" data-action="search" aria-label="${escapeAttr(window.siyuan.languages.search)}"><svg aria-hidden="true"><use xlink:href="#iconSearch"></use></svg></button>
        </div>
    </header>
    <div class="notebook-root__content">
        <main class="notebook-root__documents notebook-root__documents--${this.view}">${renderNotebookRootDocuments(this.listing.documents, this.view, this.listing)}</main>
    </div>
</div>`;
        if (this.selectedDocumentKey) {
            const selected = Array.from(this.element.querySelectorAll<HTMLElement>(".notebook-root__document"))
                .find((document) => notebookRootElementKey(document) === this.selectedDocumentKey);
            if (selected) {
                selected.classList.add("notebook-root__document--selected");
            } else {
                this.selectedDocumentKey = undefined;
            }
        }
        this.bindShellEvents();
        const documents = this.element.querySelector<HTMLElement>(".notebook-root__documents");
        if (!documents) return;
        this.createMasonryController(documents);
        this.bindDocumentEvents(documents);
        this.previewController = new DocumentCardPreviewController({
            onIdentityCreated: (identity) => {
                const document = this.listing.documents.find((item) => item.kind === "markdown" &&
                    item.notebook === identity.notebook && item.path === identity.path);
                if (!document) return;
                const previousKey = notebookRootDocumentKey({
                    kind: document.kind,
                    notebook: document.notebook,
                    id: document.documentID,
                    path: document.path,
                });
                document.documentID = identity.documentID;
                document.identityState = "valid";
                document.identityConflict = false;
                document.revision = identity.revision;
                if (this.selectedDocumentKey === previousKey) {
                    this.selectedDocumentKey = notebookRootDocumentKey({
                        kind: document.kind,
                        notebook: document.notebook,
                        id: document.documentID,
                        path: document.path,
                    });
                }
            },
        });
        this.previewController.rebind(documents.querySelectorAll<HTMLElement>(".notebook-root__document"));
        this.createDragController();
        this.scheduleTitleLayout();
    }

    private createDragController() {
        this.dragController = new NotebookRootDragController({
            element: this.element,
            notebook: this.notebookId,
            sortMode: this.listing.sortMode,
            documents: this.listing.documents,
            reload: () => this.reload(),
        });
    }

    private viewButton(view: NotebookRootView, icon: string, label: string) {
        return `<button class="block__icon block__icon--show b3-tooltips__n${view === this.view ? " notebook-root__view--active" : ""}" data-view="${view}" aria-label="${escapeAttr(label)}" aria-pressed="${view === this.view}"><svg aria-hidden="true"><use xlink:href="#${icon}"></use></svg></button>`;
    }

    private scheduleThemeRefresh() {
        if (this.destroyed || this.themeRefreshFrame) return;
        this.themeRefreshFrame = requestAnimationFrame(() => {
            this.themeRefreshFrame = 0;
            void this.previewController?.refreshAppearance();
        });
    }

    private scheduleTitleLayout() {
        if (this.destroyed || this.titleLayoutFrame) return;
        this.titleLayoutFrame = requestAnimationFrame(() => {
            this.titleLayoutFrame = 0;
            const root = this.element.querySelector<HTMLElement>(".notebook-root");
            if (root) updateNotebookRootTitleLayout(root);
            this.masonryController?.schedule();
        });
    }

    private createMasonryController(documents: HTMLElement) {
        this.masonryController?.destroy();
        this.masonryController = undefined;
        if (this.view !== "masonry") return;
        this.masonryController = new NotebookRootMasonryController(documents);
        this.masonryController.layoutNow();
    }

    private bindShellEvents() {
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
            if (view) this.switchView(view);
        }));
    }

    private selectDocument(document: HTMLElement) {
        this.selectedDocumentKey = notebookRootElementKey(document);
        this.element.querySelectorAll(".notebook-root__document--selected").forEach((item) => {
            item.classList.remove("notebook-root__document--selected");
        });
        document.classList.add("notebook-root__document--selected");
    }

    private bindDocumentEvents(documents: HTMLElement) {
        documents.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((document) => {
            const resolveSource = () => this.listing.documents.find((item) => notebookRootDocumentKey({
                kind: item.kind,
                notebook: item.notebook,
                id: item.documentID,
                path: item.path,
            }) === notebookRootElementKey(document));
            document.addEventListener("click", () => this.selectDocument(document));
            const open = () => {
                const source = resolveSource();
                if (source && this.openDocument) {
                    this.openDocument(source);
                    return;
                }
                if (document.dataset.kind === "markdown") {
                    void openMarkdownFile(this.app, this.notebookId, document.dataset.path, document.querySelector(".notebook-root__document-title")?.textContent || "Markdown");
                } else {
                    void openFileById({app: this.app, id: document.dataset.id});
                }
            };
            document.addEventListener(isMobile() ? "click" : "dblclick", open);
            document.addEventListener("keydown", (event) => {
                if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    open();
                }
            });
            if (!isMobile()) {
                document.addEventListener("contextmenu", (event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    const source = resolveSource();
                    if (!source) return;
                    this.selectDocument(document);
                    openNotebookRootContextMenu({
                        app: this.app,
                        document: source,
                        position: {x: event.clientX, y: event.clientY},
                        open,
                    });
                });
            }
        });
    }

    private switchView(view: NotebookRootView) {
        const root = this.element.querySelector<HTMLElement>(".notebook-root");
        const scroller = root?.querySelector<HTMLElement>(".notebook-root__content");
        const current = root?.querySelector<HTMLElement>(".notebook-root__documents");
        if (!root || !scroller || !current || view === this.view) return;

        const snapshot = captureNotebookRootLayoutSnapshot(scroller, current);
        const next = document.createElement("main");
        next.className = `notebook-root__documents notebook-root__documents--${view}`;
        next.innerHTML = renderNotebookRootDocuments(this.listing.documents, view, this.listing);
        hydrateNotebookRootLayout(next, snapshot);

        this.dragController?.destroy();
        this.masonryController?.destroy();
        this.masonryController = undefined;
        current.replaceWith(next);
        this.view = view;
        root.dataset.view = view;
        this.updateViewButtons(view);
        this.createMasonryController(next);
        restoreNotebookRootScrollAnchor(scroller, next, snapshot);
        this.bindDocumentEvents(next);
        this.previewController.rebind(next.querySelectorAll<HTMLElement>(".notebook-root__document"));
        this.createDragController();
        this.scheduleTitleLayout();
        setNotebookRootView(this.notebookId, view);
    }

    private updateViewButtons(view: NotebookRootView) {
        this.element.querySelectorAll<HTMLElement>(".notebook-root__views [data-view]").forEach((button) => {
            const active = button.dataset.view === view;
            button.classList.toggle("notebook-root__view--active", active);
            button.setAttribute("aria-pressed", String(active));
        });
    }
}
