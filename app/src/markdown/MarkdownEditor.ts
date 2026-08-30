import {EditorView, minimalSetup} from "codemirror";
import {Compartment, EditorState} from "@codemirror/state";
import {indentUnit} from "@codemirror/language";
/// #if !BROWSER
import {ipcRenderer} from "electron";
/// #endif
import {Model} from "../layout/Model";
import {Tab} from "../layout/Tab";
import {App} from "../index";
import {fetchPost, fetchSyncPost} from "../util/fetch";
import {confirmDialog} from "../dialog/confirmDialog";
import {escapeHtml} from "../util/escape";
import {fixWndFlex1, saveLayout, setPanelFocus} from "../layout/util";
import {createSiyuanMarkraExtension, type SiyuanMarkraExtensionOptions} from "./markraExtension";
import {
    createSiyuanMarkdownAdapter,
    resolveMarkdownCoverSource,
    resolveMarkdownImageSource,
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
import {genUUID} from "../util/genID";
import {openEmojiPanel, unicode2Emoji} from "../emoji";
import {runMarkdownTitleTransaction} from "./titleTransaction";
import {
    isGeneratedUntitledMarkdownTitle,
    MarkdownTitleComposition,
    syncMarkdownTitleEditable,
    syncMarkdownTitleElement,
    syncMarkdownTitlePresentation,
} from "./titleEditing";
import {Constants} from "../constants";
import {assetPickerMenu} from "../menus/protyle";
import {MenuItem} from "../menus/Menu";
import {fetchCoverData, renderCoverPicker} from "../protyle/header/coverData";
import {addStyle} from "../protyle/util/addStyle";
import {openTagMenu} from "../protyle/header/tagMenu";
import {MarkdownDocumentScrollController} from "./documentScroll";
import {MarkdownSlashMenuController} from "./markdownSlashMenu";
import {initialVisualMarkdownSelection} from "./markra-core/codemirror/frontmatter-preview";
import {renderMarkdownBreadcrumb} from "./breadcrumb";
import type {MarkdownDocument, MarkdownDocumentSource} from "./documentSource";
import {createWorkspaceMarkdownDocumentSource} from "./documentSource";
import {shouldInitializeMarkdownTitle} from "./initialMetadata";
import {applyMarkdownEditorShellPreferences, getMarkdownFontZoomSize, readMarkdownEditorPreferences} from "./editorPreferences";
import {
    DEFAULT_MARKDOWN_TYPEWRITER_MODE,
    normalizeMarkdownEditorSessionState,
    restoreMarkdownEditorSession,
    type MarkdownEditorSessionState,
} from "./sessionState";
import {MarkdownOutlinePublisher} from "./outlinePublisher";
import {isMarkdownStatisticsOwnerEligible} from "./statusbarOwnership";
import type {MarkdownOutlineItemWithPosition} from "./outlineModel";
import {
    codeMirrorTypewriterMode,
    MarkdownTableInteractionController,
    restoreMarkdownTableAppearances,
    showCodeMirrorLocationCue,
} from "./markra-core/codemirror";
import {MarkdownSearchController} from "./searchController";
import {countMarkdownStatistics} from "./statistics";
import {
    claimMarkdownStatusbarCounter,
    clearMarkdownStatusbarCounter,
    renderMarkdownStatusbarCounter,
} from "../layout/status";
import {executeMarkdownEditorCommand, isMarkdownTypewriterShortcut, routeMarkdownShortcut} from "./editorCommands";
import {fullscreen} from "../protyle/breadcrumb/action";
import {getMarkdownOutlineBySourceKey, MarkdownOutline} from "./MarkdownOutline";
import {matchHotKey} from "../protyle/util/hotKey";
import {editorConfigApi} from "../config/tabs/editorRuntime";
import {isMac} from "../protyle/util/compatibility";
import {MarkdownEditorRegistration, markdownEditorRegistry} from "./markdownEditorRegistry";
import {MarkdownTableAppearanceController} from "./markdownTableAppearance";
import {createMarkdownMoreMenuItems, syncMarkdownModeToggle} from "./markdownToolbar";
import {getAllModels} from "../layout/getAll";
import {
    abortMarkdownMutationAcrossRenderers,
    commitMarkdownMutationAcrossRenderers,
    isMarkdownManagementPrepareActive,
    markdownCoordinatorEditor,
    MarkdownManagementIPC,
    prepareMarkdownMutationAcrossRenderers,
} from "./managementCoordinator";
/// #if !BROWSER
import {createExternalMarkdownDocumentSource} from "./externalDocumentSource";
import {openExternalMarkdownConflictDialog} from "./externalConflictDialog";
import {ExternalEditorRegistry} from "./externalEditorRegistry";

const externalEditors = new ExternalEditorRegistry<MarkdownEditor>();
/// #endif

export const getMarkdownEditorBySourceKey = (sourceKey: string) => markdownEditorRegistry.get(sourceKey) as MarkdownEditor;

export class MarkdownEditor extends Model {
    public readonly managementID = `markdown-editor-${genUUID()}`;
    public element: HTMLElement;
    public headElement: HTMLElement;
    public notebookId: string;
    public path: string;
    public externalCapabilityId?: string;
    public view: EditorView;
    private revision: string;
    private saveTimer: number;
    private saving = false;
    private dirty = false;
    private lastSaveStatus: "saved" | "conflict" | "error" = "saved";
    private savePromise: Promise<boolean>;
    private titlePromise: Promise<boolean> = Promise.resolve(true);
    private titleTimer: number;
    private titlePlaceholder = false;
    private readonly titleComposition = new MarkdownTitleComposition();
    private preview = true;
    private destroyed = false;
    private modeCompartment = new Compartment();
    private readOnlyCompartment = new Compartment();
    private indentationCompartment = new Compartment();
    private typewriterCompartment = new Compartment();
    private contentAttributesCompartment = new Compartment();
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
    private source: MarkdownDocumentSource;
    private readonly tableAppearance: MarkdownTableAppearanceController;
    private readonly tableInteraction = new MarkdownTableInteractionController();
    private tableAppearanceSubscription?: () => void;
    private discarded = false;
    private closing = false;
    private pendingSessionState?: MarkdownEditorSessionState;
    private searchController?: MarkdownSearchController;
    private readonly outlinePublisher = new MarkdownOutlinePublisher(() => this.view?.state.doc.toString());
    private fontZoomTimer: number;
    private statisticsTimer: number;
    private recentViewTimer: number;
    private readonly statisticsOwner = {};
    private readonly registryRegistration = new MarkdownEditorRegistration(markdownEditorRegistry, this);
    private readonly restoredLayout: boolean;
    private typewriterMode = DEFAULT_MARKDOWN_TYPEWRITER_MODE;
    private typewriterModeConfigured = false;
    private outlineOpen = false;

    constructor(options: {
        app: App,
        tab?: Tab,
        element?: HTMLElement,
        notebookId?: string,
        path?: string,
        externalCapabilityId?: string,
        externalName?: string,
        externalDisplayPath?: string,
        sessionState?: MarkdownEditorSessionState,
        restoredLayout?: boolean,
    }) {
        super({app: options.app});
        this.element = options.tab?.panelElement || options.element;
        this.headElement = options.tab?.headElement;
        this.notebookId = options.notebookId || "";
        this.externalCapabilityId = options.externalCapabilityId;
        this.path = options.externalDisplayPath || options.path || options.externalName || "";
        this.pendingSessionState = options.sessionState;
        this.restoredLayout = options.restoredLayout === true;
        if (options.externalCapabilityId) {
            /// #if !BROWSER
            this.source = createExternalMarkdownDocumentSource({
                capabilityId: options.externalCapabilityId,
                readOnly: () => window.siyuan.config.readonly || window.siyuan.config.editor.readOnly,
                invoke: (channel, payload) => ipcRenderer.invoke(channel, payload),
            });
            /// #else
            throw new Error("External Markdown files are available only in the desktop app");
            /// #endif
        } else {
            this.source = this.createWorkspaceSource(options.notebookId, options.path);
        }
        this.tableAppearance = new MarkdownTableAppearanceController({
            documentKey: this.source.key,
            legacyDocumentKey: this.path,
            setExternalAppearanceRetention: async (retained) => {
                if (!this.externalCapabilityId) return;
                /// #if !BROWSER
                await ipcRenderer.invoke(Constants.SIYUAN_EXTERNAL_MARKDOWN, {
                    action: "retainAppearance",
                    capabilityId: this.externalCapabilityId,
                    retained,
                });
                /// #endif
            },
        });
        if (!this.element) {
            throw new Error("Markdown editor element is required");
        }
        addStyle(`${Constants.PROTYLE_CDN}/js/katex/katex.min.css?v=0.16.9`, "protyleKatexStyle");
        if (this.headElement && window.siyuan.config.fileTree.openFilesUseCurrentTab) {
            this.headElement.classList.add("item--unupdate");
        }
        this.renderShell();
        this.appearanceHandle = acquireMarkdownAppearance(this.element);
        void this.initialize();
    }

    private createWorkspaceSource(notebookId: string, path: string) {
        let ipc: MarkdownManagementIPC | undefined;
        /// #if BROWSER
        ipc = undefined;
        /// #else
        ipc = ipcRenderer;
        /// #endif
        return createWorkspaceMarkdownDocumentSource({
            notebookId,
            path,
            readOnly: () => window.siyuan.config.readonly || window.siyuan.config.editor.readOnly,
            request: async (url, body) => {
                if (this.restoredLayout && url === "/api/markdown/get" && !this.view) {
                    const response = await fetch(url, {method: "POST", body: JSON.stringify(body)});
                    return response.json() as Promise<{code: number, data?: Record<string, unknown>}>;
                }
                return fetchSyncPost(url, body) as Promise<{code: number, data?: Record<string, unknown>}>;
            },
            resolveImageSource: resolveMarkdownImageSource,
            saveAssets: async (files) => uploadMarkdownAssets({files: [...files], insertionOffset: 0}),
            isPrepareActive: isMarkdownManagementPrepareActive,
            prepareMutation: (ref, operationID, revision) => prepareMarkdownMutationAcrossRenderers(
                ipc,
                window.location.origin,
                ref,
                getAllModels().markdown.map(markdownCoordinatorEditor),
                operationID,
                {expectedRevision: revision, excludedEditorID: this.managementID},
            ),
            commitMutation: (operationID, lease, mutation) => commitMarkdownMutationAcrossRenderers(
                ipc,
                window.location.origin,
                operationID,
                lease,
                mutation,
                getAllModels().markdown.map(markdownCoordinatorEditor),
            ),
            abortMutation: (operationID, lease) => abortMarkdownMutationAcrossRenderers(
                ipc,
                window.location.origin,
                operationID,
                lease,
            ),
        });
    }

    private async initialize() {
        if (this.externalCapabilityId) {
            /// #if !BROWSER
            const existing = externalEditors.claim(this.externalCapabilityId, this);
            if (existing) {
                existing.focusExternalEditor();
                window.setTimeout(() => this.parent?.parent.removeTab(this.parent.id));
                return;
            }
            const retained = await ipcRenderer.invoke(Constants.SIYUAN_EXTERNAL_MARKDOWN, {
                action: "retain",
                capabilityId: this.externalCapabilityId,
            });
            if (retained?.status === "focused") {
                externalEditors.release(this.externalCapabilityId, this);
                window.setTimeout(() => this.parent?.parent.removeTab(this.parent.id));
                return;
            }
            if (retained?.status !== "ok") {
                externalEditors.release(this.externalCapabilityId, this);
                this.setStatus("error");
                return;
            }
            /// #endif
        }
        const firstWorkspaceEditor = this.source.kind === "workspace" && !getMarkdownEditorBySourceKey(this.sourceKey);
        this.registerSourceKey();
        await this.load();
        if (firstWorkspaceEditor && this.view) {
            fetchPost("/api/storage/updateRecentDocOpenTime", {
                kind: "markdown",
                notebook: this.notebookId,
                path: this.path,
            });
        }
    }

    private focusExternalEditor() {
        this.parent?.parent.switchTab(this.parent.headElement);
        this.parent?.parent.showHeading();
    }

    public save() {
        return this.flush();
    }

    public async close() {
        if (!this.parent?.parent) return;
        await this.parent.parent.removeTab(this.parent.id);
    }

    public async prepareClose() {
        if (this.source.kind !== "external" || this.discarded) return true;
        const saved = await this.flushForExit();
        if (saved) return true;
        return new Promise<boolean>((resolve) => {
            let settled = false;
            const settle = (result: boolean) => {
                if (settled) return;
                settled = true;
                if (result) this.discardChanges();
                dialog.destroy();
                resolve(result);
            };
            const dialog = new Dialog({
                title: window.siyuan.languages.externalMarkdown,
                content: `<div class="b3-dialog__content">${window.siyuan.languages.externalMarkdownUnsavedTip}</div>
<div class="b3-dialog__action"><button class="b3-button b3-button--cancel" data-action="return">${window.siyuan.languages.externalMarkdownReturnEditing}</button><div class="fn__space"></div><button class="b3-button b3-button--text" data-action="discard">${window.siyuan.languages.externalMarkdownDiscardClose}</button></div>`,
                width: "520px",
                destroyCallback: () => {
                    if (!settled) resolve(false);
                },
            });
            dialog.element.querySelector('[data-action="return"]')?.addEventListener("click", () => settle(false));
            dialog.element.querySelector('[data-action="discard"]')?.addEventListener("click", () => settle(true));
        });
    }

    public async flushForExit() {
        this.closing = true;
        try {
            return await this.flush();
        } finally {
            this.closing = false;
        }
    }

    public discardChanges() {
        this.discarded = true;
        this.dirty = false;
    }

    public async releaseExternalCapability() {
        /// #if !BROWSER
        if (!this.externalCapabilityId || !externalEditors.release(this.externalCapabilityId, this)) return;
        try {
            const result = await ipcRenderer.invoke(Constants.SIYUAN_EXTERNAL_MARKDOWN, {
                action: "release",
                capabilityId: this.externalCapabilityId,
            });
            if (result?.status !== "ok") throw new Error(result?.code || "RELEASE_FAILED");
        } catch (error) {
            externalEditors.claim(this.externalCapabilityId, this);
            throw error;
        }
        /// #endif
    }

    public refreshEditorConfig() {
        this.appearanceHandle?.refresh();
        if (!this.view) {
            return;
        }
        this.slashMenu?.close();
        const preferences = readMarkdownEditorPreferences();
        applyMarkdownEditorShellPreferences(this.element, this.titleElement, preferences);
        const anchor = this.documentScroll?.captureAnchor();
        this.view.dispatch({effects: [
            this.readOnlyCompartment.reconfigure(EditorState.readOnly.of(this.source.readOnly)),
            this.indentationCompartment.reconfigure(indentUnit.of(preferences.codeIndentation)),
            this.typewriterCompartment.reconfigure(codeMirrorTypewriterMode({
                enabled: this.typewriterMode,
                getScrollContainer: () => this.contentElement,
            })),
            this.contentAttributesCompartment.reconfigure(EditorView.contentAttributes.of({
                spellcheck: String(preferences.spellcheck),
            })),
            this.modeCompartment.reconfigure(createSiyuanMarkraExtension(this.markraExtensionOptions())),
        ]});
        this.tableInteraction.restore(this.view);
        if (anchor) this.documentScroll.restoreAnchor(anchor);
        this.slashMenu?.update();
    }

    public flush() {
        window.clearTimeout(this.saveTimer);
        if (!this.savePromise) {
            this.savePromise = Promise.all([this.flushDirty(), this.tableAppearance.flush()]).then(([saved]) => saved).finally(() => {
                this.savePromise = undefined;
            });
        }
        return trackMarkdownFlush(this.savePromise);
    }

    public get sourceKey() {
        return this.externalCapabilityId ? `external:${this.externalCapabilityId}` : `workspace:${this.notebookId}:${this.path}`;
    }

    public getRevision() {
        return this.revision;
    }

    public applyWorkspaceDocumentReference(notebookId: string, path: string, revision: string) {
        if (this.source.kind !== "workspace") return;
        this.notebookId = notebookId;
        this.path = path;
        this.revision = revision;
        this.source = this.createWorkspaceSource(notebookId, path);
        this.registerSourceKey();
        syncMarkdownTitleElement(this.titleElement, this.fileStem());
        this.updateTitle(this.fileName());
        this.renderBreadcrumb();
        this.setPreview(this.preview, false);
        saveLayout();
    }

    public hasUnsavedChanges() {
        return this.dirty || this.saving || this.lastSaveStatus !== "saved";
    }

    public getSessionState(): MarkdownEditorSessionState {
        const selection = this.view?.state.selection.main || {anchor: 0, head: 0};
        return {
            mode: this.preview ? "visual" : "source",
            selection: {anchor: selection.anchor, head: selection.head},
            scroll: this.documentScroll?.captureAnchor() || null,
            typewriterMode: this.typewriterMode,
            typewriterModeConfigured: this.typewriterModeConfigured,
        };
    }

    public captureScrollAnchor() {
        this.documentScroll?.captureAnchor();
    }

    public focusPosition(position: number, cue = true) {
        if (!this.view || !Number.isFinite(position)) return;
        const clamped = Math.max(0, Math.min(this.view.state.doc.length, Math.trunc(position)));
        this.view.dispatch({selection: {anchor: clamped}, effects: EditorView.scrollIntoView(clamped, {y: "center"})});
        if (cue) showCodeMirrorLocationCue(this.view, clamped);
        this.view.focus();
    }

    public subscribeOutline(listener: (items: readonly MarkdownOutlineItemWithPosition[]) => void) {
        return this.outlinePublisher.subscribe(listener);
    }

    public setOutlineOpen(open: boolean) {
        this.outlineOpen = open;
    }

    public isOutlineOpen() {
        return this.outlineOpen;
    }

    public openSearch(replace: boolean) {
        this.searchController?.open(replace);
    }

    public setMode(mode: "source" | "visual") {
        this.setPreview(mode === "visual");
    }

    public isReadOnly() {
        return this.source.readOnly;
    }

    public recordRecentView() {
        if (this.source.kind !== "workspace") return;
        window.clearTimeout(this.recentViewTimer);
        this.recentViewTimer = window.setTimeout(() => fetchPost("/api/storage/updateRecentDocViewTime", {
            kind: "markdown",
            notebook: this.notebookId,
            path: this.path,
        }), Constants.TIMEOUT_LOAD);
    }

    public toggleFullscreen() {
        fullscreen(this.element);
    }

    public toggleTypewriterMode() {
        this.typewriterMode = !this.typewriterMode;
        this.typewriterModeConfigured = true;
        this.refreshEditorConfig();
        saveLayout();
    }

    public updateEditorPreference(key: "justify" | "rtl", value: boolean) {
        void editorConfigApi.patch(`editor.${key}`, value);
    }

    public handleShortcut(event: KeyboardEvent) {
        return routeMarkdownShortcut(this, event, window.siyuan.config.keymap, matchHotKey);
    }

    public openOutline() {
        const existing = getMarkdownOutlineBySourceKey(this.sourceKey);
        if (existing) {
            existing.close();
            return;
        }
        this.outlineOpen = true;
        this.restoreOutline();
    }

    public restoreOutline() {
        if (!this.outlineOpen || getMarkdownOutlineBySourceKey(this.sourceKey)) return;
        if (!this.parent?.parent) return;
        const wnd = this.parent.parent.split("lr");
        wnd.element.style.width = "200px";
        wnd.element.classList.remove("fn__flex-1");
        fixWndFlex1(wnd.parent);
        wnd.addTab(new Tab({
            icon: "iconOutline",
            title: window.siyuan.languages.outline,
            callback: (tab) => tab.addModel(new MarkdownOutline({
                app: this.app,
                editor: this,
                sourceKey: this.sourceKey,
                tab,
            })),
        }), false, true);
    }

    public hideOutline(preserveState = true, isSaveLayout = false) {
        const existing = getMarkdownOutlineBySourceKey(this.sourceKey);
        if (!preserveState) this.outlineOpen = false;
        existing?.close(preserveState, isSaveLayout);
    }

    private openMoreMenu(button: HTMLElement) {
        const preferences = readMarkdownEditorPreferences();
        window.siyuan.menus.menu.remove();
        createMarkdownMoreMenuItems({
            justify: preferences.justify,
            rtl: preferences.rtl,
            typewriterMode: this.typewriterMode,
        }, {
            justify: window.siyuan.languages.justify,
            rtl: window.siyuan.languages.rtl,
            typewriterMode: window.siyuan.languages.typewriterMode,
        }, (command) => executeMarkdownEditorCommand(this, command)).forEach((item) => {
            window.siyuan.menus.menu.append(new MenuItem(item).element);
        });
        const rect = button.getBoundingClientRect();
        window.siyuan.menus.menu.popup({x: rect.right, y: rect.bottom, isLeft: true});
    }

    private async flushDirty() {
        while (this.dirty) {
            if (!await this.saveOnce()) {
                return false;
            }
        }
        return this.lastSaveStatus === "saved";
    }

    private async saveOnce(overwriteRevision?: string) {
        const view = this.view;
        if (!this.dirty) {
            return true;
        }
        if (!view) {
            return false;
        }
        this.saving = true;
        this.setStatus("saving");
        const content = view.state.sliceDoc();
        let response;
        try {
            response = await this.source.save({
                content,
                revision: this.revision,
                ...(overwriteRevision ? {overwriteRevision} : {}),
            });
        } catch {
            this.saving = false;
            this.lastSaveStatus = "error";
            if (!this.destroyed) this.setStatus("error");
            return false;
        }
        this.saving = false;
        if (response.status === "ok") {
            const document = response.document;
            this.revision = document.revision;
            this.path = document.displayPath;
            if (this.source.kind === "external") {
                this.registerSourceKey();
            }
            this.dirty = view.state.sliceDoc() !== content;
            this.lastSaveStatus = "saved";
            if (!this.destroyed) {
                this.setStatus(this.dirty ? "dirty" : "saved");
            }
            return true;
        } else if (response.status === "conflict") {
            this.lastSaveStatus = "conflict";
            if (!this.destroyed) {
                this.setStatus("conflict");
                if (this.source.kind === "external") {
                    /// #if !BROWSER
                    if (!this.closing) {
                        void openExternalMarkdownConflictDialog(response.revision, {
                            reload: () => this.reloadExternalDocument(),
                            overwrite: (revision) => this.saveOnce(revision).then(() => undefined),
                            cancel: () => undefined,
                        });
                    }
                    /// #endif
                } else {
                    confirmDialog(window.siyuan.languages.conflict, window.siyuan.languages.refresh, () => {
                        this.dirty = false;
                        void this.reloadDocument();
                    });
                }
            }
        } else {
            this.lastSaveStatus = "error";
            if (!this.destroyed) this.setStatus("error");
        }
        return false;
    }

    private async reloadExternalDocument() {
        this.dirty = false;
        await this.reloadDocument();
        if (this.view) this.lastSaveStatus = "saved";
    }

    private async reloadDocument() {
        this.slashMenu?.destroy();
        this.searchController?.destroy();
        this.slashMenu = undefined;
        this.tableAppearanceSubscription?.();
        this.tableAppearanceSubscription = undefined;
        this.view?.destroy();
        this.view = undefined;
        this.surfaceElement.innerHTML = "";
        await this.load();
    }

    public async rename(name: string) {
        return this.queueTitleTransaction(name, false);
    }

    public async duplicate() {
        if (!this.source.duplicate || !await this.flush()) return null;
        const response = await this.source.duplicate(this.revision);
        return response.status === "ok" ? response.document : null;
    }

    public async move(toNotebook: string, toParentPath: string) {
        if (!this.source.move || !await this.flush()) return false;
        const response = await this.source.move({
            revision: this.revision,
            toNotebook,
            toParentPath,
        });
        if (response.status !== "ok") return false;
        const document = response.document;
        if (this.source.kind === "external") {
            this.notebookId = toNotebook;
            this.path = document.displayPath;
            this.registerSourceKey();
            this.revision = document.revision;
            this.updateTitle(document.name);
            this.renderBreadcrumb();
            this.setPreview(this.preview, false);
            saveLayout();
        }
        return true;
    }

    private scheduleTitleInput() {
        if (!this.metadataEditable()) return;
        const title = this.normalizedTitle(this.titleElement.innerText || this.titleElement.textContent);
        if (!title) return;
        window.clearTimeout(this.titleTimer);
        this.titleTimer = window.setTimeout(() => {
            void this.queueTitleTransaction(title);
        }, 256);
    }

    private queueTitleTransaction(name: string, preserveExtension = true) {
        window.clearTimeout(this.titleTimer);
        window.clearTimeout(this.fontZoomTimer);
        const normalized = this.normalizedTitle(name);
        if (!normalized) {
            this.syncTitleElement(this.fileStem());
            return Promise.resolve(false);
        }
        this.titlePromise = this.titlePromise.catch(() => false)
            .then(() => this.commitTitle(normalized, preserveExtension));
        return this.titlePromise;
    }

    private async commitTitle(title: string, preserveExtension: boolean) {
        if (!this.metadataEditable()) {
            syncMarkdownTitleElement(this.titleElement, this.fileStem());
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
            syncMarkdownTitleElement(this.titleElement, previousTitle);
            return false;
        }
        const success = await runMarkdownTitleTransaction({
            applyTitle: (nextTitle) => this.applyMetadata({title: nextTitle}),
            flush: () => this.flush(),
            metadataTitle,
            previousTitle,
            rename: async () => {
                const response = await this.source.rename({
                    name: requestedName,
                    revision: this.revision,
                });
                if (response.status !== "ok") return false;
                const document = response.document;
                if (this.source.kind === "external") {
                    this.path = document.displayPath;
                    this.registerSourceKey();
                    this.revision = document.revision;
                    syncMarkdownTitleElement(this.titleElement, this.fileStem());
                    this.updateTitle(document.name);
                    this.renderBreadcrumb();
                    saveLayout();
                }
                return true;
            },
            renameRequired: requestedName !== this.fileName(),
        });
        if (!success) {
            this.syncTitleElement(previousTitle);
        } else {
            this.titlePlaceholder = false;
            this.titleElement.removeAttribute("placeholder");
        }
        return success;
    }

    public destroy() {
        window.clearTimeout(this.saveTimer);
        window.clearTimeout(this.titleTimer);
        window.clearTimeout(this.fontZoomTimer);
        window.clearTimeout(this.statisticsTimer);
        window.clearTimeout(this.recentViewTimer);
        clearMarkdownStatusbarCounter(this.statisticsOwner);
        this.hideOutline(false, false);
        if (this.dirty && !this.discarded && this.source.kind !== "external") {
            void this.flush();
        }
        if (this.externalCapabilityId) {
            /// #if !BROWSER
            const owned = externalEditors.release(this.externalCapabilityId, this);
            if (owned) {
                void ipcRenderer.invoke(Constants.SIYUAN_EXTERNAL_MARKDOWN, {
                    action: "release",
                    capabilityId: this.externalCapabilityId,
                }).catch(() => undefined);
            }
            /// #endif
        }
        this.destroyed = true;
        if (!this.parent && this.source.kind === "workspace") {
            fetchPost("/api/storage/updateRecentDocCloseTime", {
                kind: "markdown",
                notebook: this.notebookId,
                path: this.path,
            });
        }
        this.registryRegistration.destroy();
        this.slashMenu?.destroy();
        this.slashMenu = undefined;
        this.tableAppearanceSubscription?.();
        this.tableAppearanceSubscription = undefined;
        this.searchController?.destroy();
        this.searchController = undefined;
        this.outlinePublisher.destroy();
        const appearanceFlush = this.tableAppearance.flush();
        this.view?.destroy();
        this.documentScroll?.destroy();
        this.resizeObserver?.disconnect();
        this.appearanceHandle?.release();
        void appearanceFlush.finally(() => this.tableAppearance.destroy());
        if (process.env.NODE_ENV === "development") {
            delete (this.element as HTMLElement & {__markdownEditorView?: EditorView}).__markdownEditorView;
        }
    }

    private renderShell() {
        this.element.innerHTML = `<div class="protyle-breadcrumb markdown-editor__breadcrumb">
    <div class="protyle-breadcrumb__bar"></div>
    <span class="protyle-breadcrumb__space"></span>
    <button class="block__icon fn__flex-center ariaLabel" data-type="markdown-mode" aria-label="Markdown"><svg><use xlink:href="#iconEdit"></use></svg></button>
    <button class="block__icon fn__flex-center ariaLabel" data-type="markdown-outline" aria-label="${escapeHtml(window.siyuan.languages.outline)}"><svg><use xlink:href="#iconOutline"></use></svg></button>
    <button class="block__icon fn__flex-center ariaLabel" data-type="markdown-fullscreen" aria-label="${escapeHtml(window.siyuan.languages.fullscreen)}"><svg><use xlink:href="#iconFullscreen"></use></svg></button>
    <button class="block__icon fn__flex-center ariaLabel" data-type="markdown-more" aria-label="${escapeHtml(window.siyuan.languages.more)}"><svg><use xlink:href="#iconMore"></use></svg></button>
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
            if (action?.dataset.type === "markdown-mode") {
                this.setPreview(!this.preview);
            } else if (action?.dataset.type === "markdown-outline") {
                this.openOutline();
            } else if (action?.dataset.type === "markdown-fullscreen") {
                executeMarkdownEditorCommand(this, "toggle-fullscreen");
            } else if (action?.dataset.type === "markdown-more") {
                this.openMoreMenu(action);
                event.stopPropagation();
                event.preventDefault();
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
            if (this.handleShortcut(event)) return;
            if (isMarkdownTypewriterShortcut(event) &&
                (event.target === this.view?.contentDOM || this.view?.contentDOM.contains(event.target as Node))) {
                event.preventDefault();
                event.stopPropagation();
                executeMarkdownEditorCommand(this, "toggle-typewriter");
            } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
                event.preventDefault();
                event.stopPropagation();
                void this.save();
            } else if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "v") {
                if (executeMarkdownEditorCommand(this, "paste-plain-text", event.target)) {
                    event.preventDefault();
                    event.stopPropagation();
                }
            }
        });
        this.contentElement.addEventListener("wheel", (event: WheelEvent) => {
            const fontSize = getMarkdownFontZoomSize(event, window.siyuan.config.editor.fontSize,
                window.siyuan.config.editor.fontSizeScrollZoom, isMac());
            if (fontSize === null) return;
            event.preventDefault();
            event.stopPropagation();
            editorConfigApi.apply({...window.siyuan.config.editor, fontSize});
            window.clearTimeout(this.fontZoomTimer);
            this.fontZoomTimer = window.setTimeout(() => editorConfigApi.patch("editor.fontSize", fontSize), Constants.TIMEOUT_LOAD);
        }, {passive: false});
        this.titleElement.addEventListener("keydown", (event: KeyboardEvent) => {
            if (!this.titleComposition.acceptsKeydown(event)) return;
            if (isMarkdownSelectAll(event)) {
                event.preventDefault();
                event.stopPropagation();
                selectElementContents(this.titleElement);
            } else if (event.key === "Enter") {
                event.preventDefault();
                this.titleElement.blur();
            }
        });
        this.titleElement.addEventListener("compositionstart", () => {
            this.titleComposition.start();
        });
        this.titleElement.addEventListener("compositionend", () => {
            this.titleComposition.end();
            this.scheduleTitleInput();
        });
        this.titleElement.addEventListener("input", (event: InputEvent) => {
            if (this.titleComposition.acceptsInput(event)) this.scheduleTitleInput();
        });
        this.titleElement.addEventListener("blur", () => {
            this.titleComposition.end();
            const title = this.normalizedTitle(this.titleElement.innerText || this.titleElement.textContent);
            if (title && title !== this.fileStem()) {
                void this.queueTitleTransaction(title);
            } else if (!this.titlePlaceholder) {
                this.syncTitleElement(this.fileStem());
            }
        });
    }

    private async load() {
        let document: MarkdownDocument;
        try {
            document = await this.source.load();
        } catch {
            this.setStatus("error");
            return;
        }
        if (this.destroyed) {
            return;
        }
        this.path = document.displayPath;
        this.revision = document.revision;
        const initialMetadata = readMarkdownFrontmatter(document.content);
        this.titlePlaceholder = this.source.kind === "workspace" && isGeneratedUntitledMarkdownTitle(
            this.fileStem(),
            initialMetadata.status === "valid" ? initialMetadata.title : undefined,
            window.siyuan.languages.untitled,
        );
        await this.tableAppearance.load();
        if (this.destroyed) return;
        this.syncTitleElement(this.fileStem());
        this.updateTitle(document.name);
        this.renderBreadcrumb();
        const adapter = this.createAdapter();
        const restored = normalizeMarkdownEditorSessionState(this.pendingSessionState, document.content.length);
        const hasRestoredSession = Boolean(this.pendingSessionState);
        this.pendingSessionState = undefined;
        this.preview = restored.mode === "visual";
        this.typewriterMode = restored.typewriterMode;
        this.typewriterModeConfigured = restored.typewriterModeConfigured === true;
        const preferences = readMarkdownEditorPreferences();
        this.view = new EditorView({
            doc: document.content,
            selection: hasRestoredSession ? restored.selection : {anchor: initialVisualMarkdownSelection(document.content)},
            extensions: [
                minimalSetup,
                EditorState.lineSeparator.of(document.lineEnding),
                this.readOnlyCompartment.of(EditorState.readOnly.of(this.source.readOnly)),
                this.indentationCompartment.of(indentUnit.of(preferences.codeIndentation)),
                this.typewriterCompartment.of(codeMirrorTypewriterMode({
                    enabled: this.typewriterMode,
                    getScrollContainer: () => this.contentElement,
                })),
                this.modeCompartment.of(createSiyuanMarkraExtension(this.markraExtensionOptions(adapter))),
                this.contentAttributesCompartment.of(EditorView.contentAttributes.of({
                    spellcheck: String(preferences.spellcheck),
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
                        this.outlinePublisher.publish();
                    }
                    this.searchController?.refreshAfterViewUpdate(update);
                    if (update.docChanged || update.selectionSet || update.focusChanged) this.scheduleStatistics();
                    this.slashMenu?.update();
                }),
            ],
            parent: this.surfaceElement,
        });
        this.tableAppearanceSubscription?.();
        this.tableAppearanceSubscription = this.tableAppearance.subscribe((records) => {
            this.view?.dispatch({effects: restoreMarkdownTableAppearances.of(records)});
        });
        this.registerSourceKey();
        this.slashMenu = new MarkdownSlashMenuController(this.view, this.contentElement);
        this.searchController = new MarkdownSearchController(this.view, this.element.querySelector(".markdown-editor__breadcrumb"));
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
        restoreMarkdownEditorSession(restored, {
            configure: () => {
                this.setPreview(this.preview, false);
                this.refreshEditorConfig();
            },
            cue: (position) => showCodeMirrorLocationCue(this.view, position),
            restoreScroll: (anchor) => this.documentScroll.restoreAnchor(anchor),
        });
        this.outlinePublisher.publish();
        this.scheduleStatistics();
        const metadata = readMarkdownFrontmatter(this.view.state.doc.toString());
        if (shouldInitializeMarkdownTitle(this.source.kind, metadata, this.fileStem())) {
            this.applyMetadata({title: this.fileStem()});
        }
    }

    private createAdapter() {
        return createSiyuanMarkdownAdapter({
            app: this.app,
            documentPath: () => this.path,
            ...(this.source.kind === "external" ? {
                openLink: (target: string) => void this.source.openLink(target),
                resolveImageSource: (source: string) => this.source.resolveImageSource(source),
                saveClipboardAssets: ({files}) => this.source.saveAssets(files),
            } : {}),
        });
    }

    private markraExtensionOptions(
        adapter = this.createAdapter(),
        mode: SiyuanMarkraExtensionOptions["mode"] = this.preview ? "visual" : "source",
    ): SiyuanMarkraExtensionOptions {
        return {
            adapter,
            documentPath: () => this.path,
            getScrollContainer: () => this.contentElement,
            mode,
            tableAppearance: this.tableAppearance.pluginOptions(),
            tableInteraction: this.tableInteraction,
        };
    }

    private registerSourceKey() {
        this.registryRegistration.register(this.sourceKey);
        void this.tableAppearance.migrate(this.sourceKey);
    }

    private scheduleStatistics() {
        window.clearTimeout(this.statisticsTimer);
        if (!isMarkdownStatisticsOwnerEligible(this.view, this.element) || !document.querySelector("#status .status__counter")) {
            clearMarkdownStatusbarCounter(this.statisticsOwner);
            return;
        }
        const token = claimMarkdownStatusbarCounter(this.statisticsOwner);
        this.statisticsTimer = window.setTimeout(() => {
            if (!isMarkdownStatisticsOwnerEligible(this.view, this.element)) {
                clearMarkdownStatusbarCounter(this.statisticsOwner);
                return;
            }
            const selection = this.view.state.selection.main;
            const text = selection.empty
                ? this.view.state.doc.toString()
                : this.view.state.sliceDoc(selection.from, selection.to);
            renderMarkdownStatusbarCounter(this.statisticsOwner, token, countMarkdownStatistics(text));
        }, Constants.TIMEOUT_COUNT);
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
        syncMarkdownModeToggle(this.element, preview, {
            markdown: "Markdown",
            wysiwyg: window.siyuan.languages.wysiwyg,
        });
        if (this.view) {
            this.slashMenu?.close();
            reconfigureSiyuanMarkraExtension(
                this.view,
                this.modeCompartment,
                this.markraExtensionOptions(undefined, preview ? "visual" : "source"),
                this.documentScroll,
            );
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
        syncMarkdownTitleEditable(this.titleElement, editable);
        if (document.activeElement !== this.titleElement) {
            this.syncTitleElement(metadata.status === "valid" && metadata.title || this.fileStem());
        }
        const tags = metadata.status === "valid" ? metadata.tags : [];
        this.tagsElement.innerHTML = tags.map((tag) => `<span class="b3-chip b3-chip--middle b3-chip--pointer">${escapeHtml(tag)}${editable ? `<svg class="b3-chip__close" data-type="markdown-tag-remove" data-value="${escapeHtml(tag)}"><use xlink:href="#iconClose"></use></svg>` : ""}</span>`).join("");
        this.tagsElement.classList.toggle("fn__none", tags.length === 0);
        const icon = metadata.status === "valid" ? metadata.icon : null;
        this.iconElement.textContent = icon || "";
        this.iconElement.classList.toggle("fn__none", !icon);
        const cover = metadata.status === "valid" ? metadata.cover : null;
        const resolvedCover = cover ? this.resolveCoverSource(cover) : null;
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

    private syncTitleElement(title: string) {
        return syncMarkdownTitlePresentation(
            this.titleElement,
            title,
            window.siyuan.languages._kernel[16],
            this.titlePlaceholder && title === this.fileStem(),
        );
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
            const [saved] = await this.source.saveAssets(files);
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
            if (cover && this.resolveCoverSource(cover) && this.applyMetadata({cover})) dialog.destroy();
        });
        input.focus();
    }

    private normalizedTitle(title: string) {
        return title.replace(/[\r\n]+/gu, " ").trim();
    }

    private resolveCoverSource(source: string) {
        return this.source.kind === "external" ? this.source.resolveImageSource(source) : resolveMarkdownCoverSource(source);
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
        const parts = this.source.kind === "external"
            ? [window.siyuan.languages.externalMarkdown, ...this.path.split(/[\\/]/).filter(Boolean)]
            : [notebook?.name || this.notebookId, ...this.path.split("/").filter(Boolean)];
        this.element.querySelector(".protyle-breadcrumb__bar").innerHTML = renderMarkdownBreadcrumb(parts);
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
