/// #if !BROWSER
import {ipcRenderer} from "electron";
/// #endif
import {App} from "../index";
import {Constants} from "../constants";
import {globalCommand} from "./globalEvent/command/global";
import {commandPanel} from "./globalEvent/command/panel";
import {mountHelp} from "../util/mount";
import {selectActiveEditorContent} from "../markdown/keyboard";
import {QINGYU_CONTACT_URL} from "../util/qingyuBrand";

const NATIVE_MENU_COMMANDS = new Set([
    "config",
    "newFile",
    "recentDocs",
    "dataHistory",
    "goBack",
    "goForward",
    "globalSearch",
    "commandPanel",
    "userGuide",
    "feedback",
    "debug",
    "selectAll",
]);

let currentApp: App;
let commandListenerBound = false;

const createNativeMenuState = () => {
    const languages = window.siyuan.languages;
    const nativeMenu = languages._nativeMenu;
    const appName = ["zh-CN", "zh-TW"].includes(window.siyuan.config.appearance.lang) ? "轻语" : "QingYu";
    const labels = {
        appName,
        about: `${languages.about} ${appName}`,
        config: languages.config,
        services: nativeMenu.services,
        hide: `${languages.hide} ${appName}`,
        hideOthers: nativeMenu.hideOthers,
        showAll: nativeMenu.showAll,
        quit: languages._trayMenu.quit,
        file: nativeMenu.file,
        newFile: languages.newFile,
        recentDocs: languages.recentDocs,
        dataHistory: languages.dataHistory,
        edit: languages.edit,
        undo: languages.undo,
        redo: languages.redo,
        cut: languages.cut,
        copy: languages.copy,
        paste: languages.paste,
        pasteAndMatchStyle: languages.pasteAsPlainText,
        selectAll: languages.selectAll,
        view: nativeMenu.view,
        goBack: languages.goBack,
        goForward: languages.goForward,
        globalSearch: languages.globalSearch,
        commandPanel: languages.commandPanel,
        zoomIn: languages.zoomIn,
        zoomOut: languages.zoomOut,
        actualSize: languages.reset,
        toggleFullScreen: languages.fullscreen,
        window: nativeMenu.window,
        minimize: nativeMenu.minimize,
        zoom: languages.zoom,
        bringAllToFront: nativeMenu.bringAllToFront,
        help: languages.help,
        userGuide: languages.userGuide,
        feedback: languages.feedback,
        debug: languages.debug,
    };
    const accelerators: Record<string, string> = {};
    NATIVE_MENU_COMMANDS.forEach((command) => {
        const accelerator = window.siyuan.config.keymap.general[command]?.custom;
        if (accelerator) {
            accelerators[command] = accelerator;
        }
    });
    return {
        ready: true,
        readonly: window.siyuan.config.readonly,
        labels,
        accelerators,
    };
};

const executeNativeMenuCommand = (command: string) => {
    if (!NATIVE_MENU_COMMANDS.has(command) || !currentApp) {
        return;
    }
    if (command === "commandPanel") {
        commandPanel(currentApp);
    } else if (command === "userGuide") {
        mountHelp();
    } else if (command === "feedback") {
        window.open(QINGYU_CONTACT_URL);
    } else if (command === "debug") {
        ipcRenderer.send(Constants.SIYUAN_CMD, "openDevTools");
    } else if (command === "selectAll") {
        selectActiveEditorContent();
    } else {
        globalCommand(command, currentApp);
    }
};

export const initNativeMenu = (app: App) => {
    /// #if !BROWSER
    if (window.siyuan.config.system.os !== "darwin") {
        return;
    }
    currentApp = app;
    if (!commandListenerBound) {
        ipcRenderer.on(Constants.SIYUAN_NATIVE_MENU_COMMAND, (event, command) => {
            if (typeof command === "string") {
                executeNativeMenuCommand(command);
            }
        });
        commandListenerBound = true;
    }
    ipcRenderer.send(Constants.SIYUAN_NATIVE_MENU_STATE, createNativeMenuState());
    /// #endif
};
