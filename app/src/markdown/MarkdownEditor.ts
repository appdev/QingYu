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
import {
    createSiyuanMarkdownAdapter,
    resolveMarkdownCoverSource,
    uploadMarkdownAssets,
} from "./siyuanAdapter";
import {trackMarkdownFlush} from "./saveBarrier";
import {acquireMarkdownAppearance, type MarkdownAppearanceHandle} from "./appearance/themeResolver";
import {reconfigureSiyuanMarkraExtension} from "./markdownEditorExtension";
import {isMarkdownSelectAll, selectElementContents} from "./keyboard";
import {
    readMarkdownFrontmatter,
    type MarkdownFrontmatterMetadataPatch,
    upsertMarkdownFrontmatterMetadata,
} from "./markra-core/markdown/frontmatter";
import {Dialog} from "../dialog";
import {getRandom, isMobile} from "../util/functions";
import {openEmojiPanel, unicode2Emoji} from "../emoji";
import {runMarkdownTitleTransaction} from "./titleTransaction";
import {Constants} from "../constants";
import {assetPickerMenu} from "../menus/protyle";
import {fetchCoverData, renderCoverPicker} from "../protyle/header/coverData";
import {openTagMenu} from "../protyle/header/tagMenu";
import {MarkdownDocumentScrollController} from "./documentScroll";
import {MarkdownSlashMenuController} from "./markdownSlashMenu";

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
    private titlePromise: Promise<boolean> = Promise.resolve(true);
    private titleTimer: number;
    private preview = true;
    private destroyed = false;
    private modeCompartment = new Compartment();
    private titleElement: HTMLElement;
    private contentElement: HTMLElement;
    private metadataElement: HTMLElement;
    private coverElement: HTMLElement;
    private iconElement: HTMLElement;
    private tagsElement: HTMLElement;
    private surfaceElement: HTMLElement;
    private statusElement: HTMLElement;
    private appearanceHandle: MarkdownAppearanceHandle;
    private resizeObserver: ResizeObserver;
    private documentScroll: MarkdownDocumentScrollController;
    private slashMenu?: MarkdownSlashMenuController;

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
        this.appearanceHandle = acquireMarkdownAppearance(this.element);
        this.load();
    }

    public save() {
        return this.flush();
    }

    public refreshEditorConfig() {
        this.appearanceHandle?.refresh();
        if (!this.view) {
            return;
        }
        this.slashMenu?.close();
        reconfigureSiyuanMarkraExtension(this.view, this.modeCompartment, {
            adapter: createSiyuanMarkdownAdapter({
                app: this.app,
                documentPath: () => this.path,
            }),
            documentPath: () => this.path,
            getScrollContainer: () => this.contentElement,
            mode: this.preview ? "visual" : "source",
        }, this.documentScroll);
        this.slashMenu?.update();
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
                    this.slashMenu?.destroy();
                    this.slashMenu = undefined;
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
        return this.queueTitleTransaction(name, false);
    }

    private queueTitleTransaction(name: string, preserveExtension = true) {
        window.clearTimeout(this.titleTimer);
        const normalized = this.normalizedTitle(name);
        if (!normalized) {
            this.titleElement.textContent = this.fileStem();
            return Promise.resolve(false);
        }
        this.titlePromise = this.titlePromise.catch(() => false)
            .then(() => this.commitTitle(normalized, preserveExtension));
        return this.titlePromise;
    }

    private async commitTitle(title: string, preserveExtension: boolean) {
        if (!this.metadataEditable()) {
            this.titleElement.textContent = this.fileStem();
            return false;
        }
        const previousTitle = this.fileStem();
        const requestedName = preserveExtension ? `${title}${this.fileExtension()}` : title;
        const lowerName = requestedName.toLowerCase();
        const requestedExtension = lowerName.endsWith(".markdown")
            ? ".markdown"
            : lowerName.endsWith(".md") ? ".md" : "";
        const metadataTitle = preserveExtension || !requestedExtension
            ? title
            : requestedName.slice(0, -requestedExtension.length);
        if (!metadataTitle) {
            this.titleElement.textContent = previousTitle;
            return false;
        }
        const success = await runMarkdownTitleTransaction({
            applyTitle: (nextTitle) => this.applyMetadata({title: nextTitle}),
            flush: () => this.flush(),
            metadataTitle,
            previousTitle,
            rename: async () => {
                const response = await fetchSyncPost("/api/markdown/rename", {
                    notebook: this.notebookId,
                    path: this.path,
                    name: requestedName,
                });
                if (response.code !== 0) return false;
                const document = response.data as IMarkdownDocument;
                this.path = document.path;
                this.revision = document.revision;
                this.titleElement.textContent = this.fileStem();
                this.updateTitle(document.name);
                this.renderBreadcrumb();
                saveLayout();
                return true;
            },
            renameRequired: requestedName !== this.fileName(),
        });
        if (!success) {
            this.titleElement.textContent = previousTitle;
        }
        return success;
    }

    public destroy() {
        window.clearTimeout(this.saveTimer);
        window.clearTimeout(this.titleTimer);
        if (this.dirty) {
            void this.flush();
        }
        this.destroyed = true;
        this.slashMenu?.destroy();
        this.slashMenu = undefined;
        this.view?.destroy();
        this.documentScroll?.destroy();
        this.resizeObserver?.disconnect();
        this.appearanceHandle?.release();
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
        <div class="protyle-background markdown-editor__metadata protyle-background--enable">
            <div class="protyle-background__img markdown-editor__cover fn__none">
                <img alt="">
                <div class="protyle-icons">
                    <span class="protyle-icon protyle-icon--first" style="position: relative;overflow: hidden"><input aria-label="${escapeHtml(window.siyuan.languages.upload)}" class="ariaLabel b3-form__upload markdown-editor__cover-upload" type="file" accept="image/*"><svg><use xlink:href="#iconUpload"></use></svg></span>
                    <span class="protyle-icon ariaLabel" data-type="markdown-cover-link" aria-label="${escapeHtml(window.siyuan.languages.link)}"><svg><use xlink:href="#iconLink"></use></svg></span>
                    <span class="protyle-icon ariaLabel" data-type="markdown-cover-asset" aria-label="${escapeHtml(window.siyuan.languages.assets)}"><svg><use xlink:href="#iconImage"></use></svg></span>
                    <span class="protyle-icon ariaLabel" data-type="markdown-cover-built-in" aria-label="${escapeHtml(window.siyuan.languages.builtIn)}"><svg><use xlink:href="#iconRefresh"></use></svg></span>
                    <span class="protyle-icon protyle-icon--last ariaLabel" data-type="markdown-cover-remove" aria-label="${escapeHtml(window.siyuan.languages.remove)}"><svg><use xlink:href="#iconTrashcan"></use></svg></span>
                </div>
            </div>
            <div class="protyle-background__ia">
                <div class="protyle-background__icon markdown-editor__icon" data-type="markdown-icon"></div>
                <div class="b3-chips b3-chips__doctag markdown-editor__tags"></div>
                <div class="protyle-background__action markdown-editor__actions">
                    <button class="b3-button b3-button--cancel" data-type="markdown-tag"><svg><use xlink:href="#iconTags"></use></svg>${window.siyuan.languages.addTag}</button>
                    <button class="b3-button b3-button--cancel" data-type="markdown-icon"><svg><use xlink:href="#iconEmoji"></use></svg>${window.siyuan.languages.addIcon}</button>
                    <button class="b3-button b3-button--cancel" data-type="markdown-cover"><svg><use xlink:href="#iconImage"></use></svg>${window.siyuan.languages.titleBg}</button>
                </div>
            </div>
        </div>
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
        this.contentElement = this.element.querySelector(".markdown-editor__content");
        this.metadataElement = this.element.querySelector(".markdown-editor__metadata");
        this.metadataElement.classList.toggle("protyle-background--mobileshow", isMobile());
        this.coverElement = this.element.querySelector(".markdown-editor__cover");
        this.iconElement = this.element.querySelector(".markdown-editor__icon");
        this.tagsElement = this.element.querySelector(".markdown-editor__tags");
        this.titleElement = this.element.querySelector(".protyle-title__input");
        this.surfaceElement = this.element.querySelector(".markdown-editor__surface");
        this.statusElement = this.element.querySelector(".markdown-editor__status");
        this.resizeObserver = new ResizeObserver(() => this.updateLayout());
        this.resizeObserver.observe(this.element);
        this.updateLayout();
        this.element.addEventListener("click", (event) => {
            if (this.element.parentElement?.parentElement) {
                setPanelFocus(this.element.parentElement.parentElement);
            }
            const target = event.target as HTMLElement;
            const button = target.closest("button");
            const action = button || target.closest<HTMLElement>("[data-type]");
            if (action?.dataset.type === "markdown-source") {
                this.setPreview(false);
            } else if (action?.dataset.type === "markdown-preview") {
                this.setPreview(true);
            } else if (action?.dataset.type === "markdown-tag") {
                this.openTagDialog(action);
            } else if (action?.dataset.type === "markdown-icon") {
                this.openIconDialog(action);
            } else if (action?.dataset.type === "markdown-cover") {
                void this.addRandomCover();
            } else if (action?.dataset.type === "markdown-cover-link") {
                this.openCoverLinkDialog();
            } else if (action?.dataset.type === "markdown-cover-asset") {
                this.openCoverAssetMenu(action);
            } else if (action?.dataset.type === "markdown-cover-built-in") {
                this.openBuiltInCoverDialog();
            } else if (action?.dataset.type === "markdown-cover-remove") {
                this.applyMetadata({cover: ""});
            } else if (action?.dataset.type === "markdown-tag-remove") {
                this.removeTag(action.dataset.value || "");
            }
        });
        this.element.querySelector<HTMLInputElement>(".markdown-editor__cover-upload").addEventListener("change", (event) => {
            const input = event.currentTarget as HTMLInputElement;
            if (input.files?.length) {
                void this.uploadCoverFiles(Array.from(input.files)).finally(() => input.value = "");
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
        this.titleElement.addEventListener("input", () => {
            if (!this.metadataEditable()) return;
            const title = this.normalizedTitle(this.titleElement.innerText || this.titleElement.textContent);
            if (!title) return;
            this.applyMetadata({title});
            window.clearTimeout(this.titleTimer);
            this.titleTimer = window.setTimeout(() => {
                void this.queueTitleTransaction(title);
            }, 256);
        });
        this.titleElement.addEventListener("blur", () => {
            const title = this.normalizedTitle(this.titleElement.innerText || this.titleElement.textContent);
            if (title && title !== this.fileStem()) {
                void this.queueTitleTransaction(title);
            } else {
                this.titleElement.textContent = this.fileStem();
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
        this.titleElement.textContent = this.fileStem();
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
                    getScrollContainer: () => this.contentElement,
                    mode: "visual",
                })),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        this.dirty = true;
                        this.setStatus("dirty");
                        this.renderMetadata();
                        if (!this.preview) {
                            this.scheduleSourceTitleSync();
                        }
                        this.scheduleSave();
                    }
                    this.slashMenu?.update();
                }),
            ],
            parent: this.surfaceElement,
        });
        this.slashMenu = new MarkdownSlashMenuController(this.view, this.contentElement);
        this.slashMenu.update();
        this.documentScroll = new MarkdownDocumentScrollController(() => this.view, this.contentElement);
        if (process.env.NODE_ENV === "development") {
            Object.defineProperty(this.element, "__markdownEditorView", {
                configurable: true,
                value: this.view,
            });
        }
        this.setStatus("saved");
        this.renderMetadata();
        this.setPreview(true, false);
        const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
        if (metadata.status === "none" || metadata.status === "valid" && metadata.title !== this.fileStem()) {
            this.applyMetadata({title: this.fileStem()});
        }
    }

    private scheduleSave() {
        window.clearTimeout(this.saveTimer);
        this.saveTimer = window.setTimeout(() => {
            void this.save();
        }, 800);
    }

    private scheduleSourceTitleSync() {
        const metadata = this.view && readMarkdownFrontmatter(this.view.state.doc.toString());
        if (!metadata || metadata.status !== "valid" || !metadata.title || metadata.title === this.fileStem()) return;
        window.clearTimeout(this.titleTimer);
        this.titleTimer = window.setTimeout(() => {
            void this.queueTitleTransaction(metadata.title);
        }, 256);
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
            this.slashMenu?.close();
            reconfigureSiyuanMarkraExtension(this.view, this.modeCompartment, {
                adapter: createSiyuanMarkdownAdapter({
                    app: this.app,
                    documentPath: () => this.path,
                }),
                documentPath: () => this.path,
                getScrollContainer: () => this.contentElement,
                mode: preview ? "visual" : "source",
            }, this.documentScroll);
            this.slashMenu?.update();
            if (focus) {
                this.view.focus();
            }
        }
    }

    private metadataEditable() {
        return !window.siyuan.config.readonly && !window.siyuan.config.editor.readOnly;
    }

    private applyMetadata(patch: MarkdownFrontmatterMetadataPatch) {
        if (!this.view || !this.metadataEditable()) return false;
        const source = this.view.state.doc.toString();
        const result = upsertMarkdownFrontmatterMetadata(source, patch);
        if (!result.ok) {
            this.setStatus("error");
            this.renderMetadata();
            return false;
        }
        if (result.changed) {
            this.view.dispatch({changes: {from: 0, to: source.length, insert: result.source}});
        } else {
            this.renderMetadata();
        }
        return true;
    }

    private renderMetadata() {
        if (!this.view) return;
        const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
        const editable = this.metadataEditable() && metadata.status !== "malformed";
        this.metadataElement.classList.toggle("protyle-background--enable", editable);
        this.titleElement.setAttribute("contenteditable", editable ? "true" : "false");
        if (document.activeElement !== this.titleElement) {
            this.titleElement.textContent = metadata.status === "valid" && metadata.title || this.fileStem();
        }
        const tags = metadata.status === "valid" ? metadata.tags : [];
        this.tagsElement.innerHTML = tags.map((tag) => `<span class="b3-chip b3-chip--middle b3-chip--pointer">${escapeHtml(tag)}${editable ? `<svg class="b3-chip__close" data-type="markdown-tag-remove" data-value="${escapeHtml(tag)}"><use xlink:href="#iconClose"></use></svg>` : ""}</span>`).join("");
        this.tagsElement.classList.toggle("fn__none", tags.length === 0);
        const icon = metadata.status === "valid" ? metadata.icon : null;
        this.iconElement.textContent = icon || "";
        this.iconElement.classList.toggle("fn__none", !icon);
        const cover = metadata.status === "valid" ? metadata.cover : null;
        const resolvedCover = cover ? resolveMarkdownCoverSource(cover) : null;
        this.coverElement.classList.toggle("fn__none", !resolvedCover);
        const image = this.coverElement.querySelector("img") as HTMLImageElement;
        if (resolvedCover) image.src = resolvedCover;
        this.metadataElement.querySelector<HTMLElement>('.markdown-editor__actions [data-type="markdown-tag"]')?.classList.remove("fn__none");
        this.metadataElement.querySelector<HTMLElement>('.markdown-editor__actions [data-type="markdown-icon"]')?.classList.toggle("fn__none", Boolean(icon));
        this.metadataElement.querySelector<HTMLElement>('.markdown-editor__actions [data-type="markdown-cover"]')?.classList.toggle("fn__none", Boolean(resolvedCover));
        if (!editable) {
            this.metadataElement.querySelectorAll<HTMLButtonElement>("button").forEach((button) => button.disabled = true);
        } else {
            this.metadataElement.querySelectorAll<HTMLButtonElement>("button").forEach((button) => button.disabled = false);
        }
    }

    private openTagDialog(target: HTMLElement) {
        if (!this.metadataEditable()) return;
        openTagMenu({
            target,
            getCurrentTags: () => {
                const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
                return metadata.status === "valid" ? [...metadata.tags] : [];
            },
            toggleTag: (tag, done) => {
                const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
                const tags = metadata.status === "valid" ? [...metadata.tags] : [];
                const index = tags.indexOf(tag);
                if (index > -1) {
                    tags.splice(index, 1);
                } else {
                    tags.push(tag);
                }
                if (this.applyMetadata({tags})) done();
            },
        });
    }

    private removeTag(tag: string) {
        const metadata = this.view && readMarkdownFrontmatter(this.view.state.doc.toString());
        if (!metadata || metadata.status !== "valid") return;
        this.applyMetadata({tags: metadata.tags.filter((item) => item !== tag)});
    }

    private openIconDialog(target: HTMLElement) {
        if (!this.metadataEditable()) return;
        const rect = target.getBoundingClientRect();
        openEmojiPanel("", "av", {x: rect.left, y: rect.bottom, h: rect.height, w: rect.width}, (unicode) => {
            this.applyMetadata({icon: unicode2Emoji(unicode)});
        }, undefined, {dynamic: true, custom: true});
    }

    private async uploadCoverFiles(files: File[]) {
        if (!this.metadataEditable()) return;
        try {
            const [saved] = await uploadMarkdownAssets({files, insertionOffset: 0});
            if (saved) this.applyMetadata({cover: saved.markdownDestination});
        } catch {
            this.setStatus("error");
        }
    }

    private async applyBuiltInCover(name: string) {
        try {
            const response = await fetch(`/appearance/covers/${encodeURIComponent(name)}`);
            if (!response.ok) throw new Error(`Cover download failed with HTTP ${response.status}`);
            const file = new File([await response.blob()], name);
            await this.uploadCoverFiles([file]);
        } catch {
            this.setStatus("error");
        }
    }

    private async addRandomCover() {
        if (!this.metadataEditable()) return;
        const coverData = await fetchCoverData();
        if (!coverData?.allCovers.length) {
            this.setStatus("error");
            return;
        }
        const cover = coverData.allCovers[getRandom(0, coverData.allCovers.length - 1)];
        await this.applyBuiltInCover(cover.file);
    }

    private openBuiltInCoverDialog() {
        if (!this.metadataEditable()) return;
        const dialog = new Dialog({
            title: window.siyuan.languages.builtIn,
            content: "<div class=\"b3-cards\" style=\"padding:16px;justify-content:center;align-items:center;min-height:200px\"><img src=\"/stage/loading-pure.svg\" style=\"width:64px;height:64px\"></div>",
            width: isMobile() ? "92vw" : "912px",
            height: isMobile() ? "80vh" : "70vh",
        });
        dialog.element.setAttribute("data-key", Constants.DIALOG_BACKGROUNDRANDOM);
        void renderCoverPicker(dialog, (name) => void this.applyBuiltInCover(name)).then((rendered) => {
            if (!rendered) {
                dialog.destroy();
                this.setStatus("error");
            }
        }).catch(() => {
            dialog.destroy();
            this.setStatus("error");
        });
    }

    private openCoverAssetMenu(target: HTMLElement) {
        if (!this.metadataEditable()) return;
        const rect = target.getBoundingClientRect();
        assetPickerMenu({
            x: target.parentElement.getBoundingClientRect().right,
            y: rect.bottom + 8,
            isLeft: true,
        }, (url) => this.applyMetadata({cover: url}), Constants.SIYUAN_ASSETS_IMAGE);
    }

    private openCoverLinkDialog() {
        if (!this.metadataEditable()) return;
        const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
        const currentCover = metadata.status === "valid" ? metadata.cover || "" : "";
        const dialog = new Dialog({
            title: window.siyuan.languages.link,
            content: `<div class="b3-dialog__content"><input class="b3-text-field fn__block" value="${escapeHtml(currentCover)}"></div><div class="b3-dialog__action"><button class="b3-button b3-button--cancel">${window.siyuan.languages.cancel}</button><div class="fn__space"></div><button class="b3-button b3-button--text">${window.siyuan.languages.confirm}</button></div>`,
            width: isMobile() ? "92vw" : "520px",
        });
        dialog.element.setAttribute("data-key", Constants.DIALOG_BACKGROUNDLINK);
        const input = dialog.element.querySelector("input") as HTMLInputElement;
        const buttons = dialog.element.querySelectorAll<HTMLButtonElement>(".b3-dialog__action .b3-button");
        buttons[0].addEventListener("click", () => dialog.destroy());
        buttons[1].addEventListener("click", () => {
            const cover = input.value.trim();
            if (cover && resolveMarkdownCoverSource(cover) && this.applyMetadata({cover})) dialog.destroy();
        });
        input.focus();
    }

    private normalizedTitle(title: string) {
        return title.replace(/[\r\n]+/gu, " ").trim();
    }

    private updateLayout() {
        const mobile = isMobile();
        this.element.dataset.markdownPlatform = mobile ? "mobile" : "desktop";
        let left = 24;
        let right = 16;
        if (!mobile) {
            const width = this.element.clientWidth;
            if (!window.siyuan.config.editor.fullWidth) {
                let padding = (width - Constants.SIZE_EDITOR_WIDTH) / 2;
                if (padding > 96) {
                    if (padding > Constants.SIZE_EDITOR_WIDTH) {
                        padding = width * .382 / 1.382;
                    }
                    left = Math.ceil(padding);
                    right = left;
                } else if (width > Constants.SIZE_EDITOR_WIDTH) {
                    left = 96;
                    right = 96;
                }
            } else if (width > Constants.SIZE_EDITOR_WIDTH) {
                left = 96;
                right = 96;
            }
        }
        const iaElement = this.metadataElement.querySelector<HTMLElement>(".protyle-background__ia");
        iaElement.style.marginLeft = `${left}px`;
        iaElement.style.marginRight = `${right}px`;
        this.titleElement.parentElement.style.margin = `16px ${right}px 0 ${left}px`;
        this.element.querySelector<HTMLElement>(".markdown-editor__body").style.padding = `16px ${right}px 0 ${left}px`;
        this.element.style.setProperty("--b3-width-protyle", `${this.element.clientWidth}px`);
        this.element.style.setProperty("--b3-width-protyle-content", `${this.element.clientWidth}px`);
        this.element.style.setProperty("--b3-width-protyle-wysiwyg", `${Math.max(0, this.element.clientWidth - left - right)}px`);
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

    private fileExtension() {
        const name = this.fileName();
        const index = name.lastIndexOf(".");
        return index > 0 ? name.slice(index) : ".md";
    }

    private fileStem() {
        return this.fileName().slice(0, -this.fileExtension().length);
    }
}
