import {Constants} from "../constants";
import {Menus} from "../menus";
import {Model} from "../layout/Model";
import "../assets/scss/base.scss";
import {initBlockPopover} from "../block/popover";
import {addScript, addScriptSync} from "../protyle/util/addScript";
import {genUUID} from "../util/genID";
import {fetchGet, fetchPost} from "../util/fetch";
import {addBaseURL, getDocDisplayName, redirectToCheckAuth, setNoteBook} from "../util/pathName";
import {openFileById} from "../editor/util";
import {
    processSync,
    progressBackgroundTask,
    progressLoading,
    progressStatus,
    setDefRefCount,
    setRefDynamicText,
    transactionError
} from "../dialog/processSystem";
import {initMessage} from "../dialog/message";
import {getAllModels, getAllTabs} from "../layout/getAll";
import {refreshMarkdownEditorsForConfigMessage} from "../markdown/configMessageRefresh";
import {handleMarkdownSaveBarrier} from "../markdown/saveBarrier";
import {getLocalStorage, setStorageVal} from "../protyle/util/compatibility";
import {init} from "./init";
import {loadPlugins, reloadPlugin} from "../plugin/loader";
import {hideAllElements} from "../protyle/ui/hideElements";
import {reloadEmoji} from "../emoji";
import {appearanceConfigApi} from "../config/tabs/appearanceRuntime";
import {renderSnippet} from "../config/util/snippets";
import {setBodyHighlight} from "../util/assets";
import {reloadSync} from "../util/reloadSync";
import {setTitle} from "../util/processTitle";
import {
    createMarkdownManagementRuntimeEventController,
    markdownReferenceFromInitData,
    markdownManagementEventFromWebSocket,
} from "../markdown/documentManagement";
import {installMarkdownManagementRendererCoordinator, markdownCoordinatorEditor} from "../markdown/managementCoordinator";
import {applyMarkdownTableAppearanceEvent} from "../markdown/markdownTableAppearance";
import {ipcRenderer} from "electron";

const markdownManagementEvents = createMarkdownManagementRuntimeEventController(() => {
    const closedTabs = window.siyuan.storage[Constants.LOCAL_CLOSED_TABS] || [];
    const lazyEditors = getAllTabs("MarkdownEditor").flatMap((tab) => {
        if (tab.model) return [];
        const reference = markdownReferenceFromInitData(tab.headElement?.getAttribute("data-initdata"));
        return reference ? [{
            notebookId: reference.notebook,
            path: reference.path,
            close: () => tab.parent.removeTab(tab.id).then(() => undefined),
        }] : [];
    });
    return {
        editors: [...getAllModels().markdown.map((editor) => ({
            get notebookId() { return editor.notebookId; },
            get path() { return editor.path; },
            getRevision: () => editor.getRevision(),
            applyWorkspaceDocumentReference: (notebook: string, path: string, revision: string) =>
                editor.applyWorkspaceDocumentReference(notebook, path, revision),
            close: () => editor.parent.parent.removeTab(editor.parent.id).then(() => undefined),
        })), ...lazyEditors],
        closedTabs,
        persistClosedTabs: (layouts: readonly unknown[]) => setStorageVal(Constants.LOCAL_CLOSED_TABS, layouts),
    };
});

class App {
    public plugins: import("../plugin").Plugin[] = [];
    public appId: string;

    constructor() {
        addBaseURL();
        this.appId = Constants.SIYUAN_APPID;

        const mainWs = new Model({app: this});
        mainWs.connect({
            id: genUUID(),
            type: "main",
            msgCallback: (data) => {
                    this.plugins.forEach((plugin) => {
                        plugin.eventBus.emit("ws-main", data);
                    });
                    if (data) {
                        switch (data.cmd) {
                            case "logoutAuth":
                                redirectToCheckAuth();
                                break;
                            case "setAppearance":
                                appearanceConfigApi.apply(data.data);
                                break;
                            case "setSnippet":
                                window.siyuan.config.snippet = data.data;
                                renderSnippet();
                                break;
                            case "setDefRefCount":
                                setDefRefCount(data.data);
                                break;
                            case "setRefDynamicText":
                                setRefDynamicText(data.data);
                                break;
                            case "reloadPlugin":
                                reloadPlugin(this, data.data);
                                break;
                            case "reloadEmojiConf":
                                reloadEmoji();
                                break;
                            case "reloaddoc":
                                reloadSync(this, {upsertRootIDs: [data.data], removeRootIDs: []}, false, false, true);
                                break;
                            case "flushMarkdownForAssetScan":
                                void handleMarkdownSaveBarrier(data.data, mainWs.sessionId, getAllModels().markdown);
                                break;
                            case "markdownTableAppearance":
                                applyMarkdownTableAppearanceEvent(data.data);
                                break;
                            case "syncMergeResult":
                                reloadSync(this, data.data);
                                break;
                            case "readonly":
                                window.siyuan.config.editor.readOnly = data.data;
                                refreshMarkdownEditorsForConfigMessage(data.cmd, getAllModels().markdown);
                                hideAllElements(["util"]);
                                break;
                            case "setConf":
                                window.siyuan.config = data.data;
                                refreshMarkdownEditorsForConfigMessage(data.cmd, getAllModels().markdown);
                                break;
                            case "progress":
                                progressLoading(data);
                                break;
                            case "setLocalStorageVal":
                                if (window.siyuan.storage) {
                                    window.siyuan.storage[data.data.key] = data.data.val;
                                }
                                break;
                            case "setLocalStorageVals":
                                Object.keys(data.data.keyVals).forEach((k) => {
                                    window.siyuan.storage[k] = data.data.keyVals[k];
                                });
                                break;
                            case "removeLocalStorageVal":
                                delete window.siyuan.storage[data.data.key];
                                break;
                            case "removeLocalStorageVals":
                                data.data.keys.forEach((k: string) => {
                                    delete window.siyuan.storage[k];
                                });
                                break;
                            case "rename":
                                getAllTabs().forEach((tab) => {
                                    if (tab.headElement) {
                                        const initTab = tab.headElement.getAttribute("data-initdata");
                                        if (initTab) {
                                            const initTabData = JSON.parse(initTab);
                                            if (initTabData.instance === "Editor" && initTabData.rootId === data.data.id) {
                                                tab.updateTitle(getDocDisplayName(data.data.title, data.data.empty));
                                            }
                                        }
                                    }
                                });
                                break;
                            case "createMarkdown":
                            case "saveMarkdown":
                            case "renameMarkdown":
                            case "removeMarkdown":
                            case "sortMarkdown":
                            case "purgeMarkdown": {
                                markdownManagementEvents.handle(markdownManagementEventFromWebSocket(data));
                                break;
                            }
                            case "closeBox":
                            case "removeBox":
                                getAllTabs().forEach((tab) => {
                                    if (tab.headElement) {
                                        const initTab = tab.headElement.getAttribute("data-initdata");
                                        if (initTab) {
                                            const initTabData = JSON.parse(initTab);
                                            if (initTabData.instance === "Editor" && data.data.box === initTabData.notebookId) {
                                                tab.parent.removeTab(tab.id);
                                            }
                                        }
                                    }
                                });
                                break;
                            case "removeDoc":
                                getAllTabs().forEach((tab) => {
                                    if (tab.headElement) {
                                        const initTab = tab.headElement.getAttribute("data-initdata");
                                        if (initTab) {
                                            const initTabData = JSON.parse(initTab);
                                            if (initTabData.instance === "Editor" && data.data.ids.includes(initTabData.rootId)) {
                                                tab.parent.removeTab(tab.id);
                                            }
                                        }
                                    }
                                });
                                break;
                            case "statusbar":
                                progressStatus(data);
                                break;
                            case "txerr":
                                transactionError(data.msg);
                                break;
                            case "syncing":
                                processSync(data, this.plugins);
                                break;
                            case "backgroundtask":
                                progressBackgroundTask(data.data.tasks);
                                break;
                            case "refreshtheme":
                                if ((window.siyuan.config.appearance.mode === 1 && window.siyuan.config.appearance.themeDark !== "midnight") || (window.siyuan.config.appearance.mode === 0 && window.siyuan.config.appearance.themeLight !== "daylight")) {
                                    (document.getElementById("themeStyle") as HTMLLinkElement).href = data.data.theme;
                                } else {
                                    (document.getElementById("themeDefaultStyle") as HTMLLinkElement).href = data.data.theme;
                                }
                                break;
                            case "openFileById":
                                openFileById({app: this, id: data.data.id, action: [Constants.CB_GET_FOCUS]});
                                break;
                        }
                    }
                }
        });

        window.siyuan = {
            zIndex: 10,
            reqIds: {},
            backStack: [],
            layout: {},
            dialogs: [],
            blockPanels: [],
            closedTabs: [],
            ctrlIsPressed: false,
            altIsPressed: false,
            ws: mainWs,
        };
        fetchPost("/api/system/getConf", {}, async (response) => {
            addScriptSync(`${Constants.PROTYLE_CDN}/js/lute/lute.min.js?v=${Constants.SIYUAN_VERSION}`, "protyleLuteScript");
            addScript(`${Constants.PROTYLE_CDN}/js/protyle-html.js?v=${Constants.SIYUAN_VERSION}`, "protyleWcHtmlScript");
            window.siyuan.config = response.data.conf;
            setBodyHighlight();
            window.siyuan.isPublish = response.data.isPublish;
            await loadPlugins(this);
            getLocalStorage(() => {
                fetchGet(`/appearance/langs/${window.siyuan.config.appearance.lang}.json?v=${Constants.SIYUAN_VERSION}`, (lauguages: IObject) => {
                    window.siyuan.languages = lauguages;
                    window.siyuan.menus = new Menus(this);
                    init(this);
                    setTitle("", true);
                    initMessage();
                });
            });
        });
        setNoteBook();
        initBlockPopover(this);
    }
}

new App();
installMarkdownManagementRendererCoordinator(ipcRenderer, window.location.origin,
    () => getAllModels().markdown.map(markdownCoordinatorEditor));
