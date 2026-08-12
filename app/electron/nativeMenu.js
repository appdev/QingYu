// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

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

const NATIVE_MENU_LABEL_KEYS = [
    "appName",
    "about",
    "config",
    "services",
    "hide",
    "hideOthers",
    "showAll",
    "quit",
    "file",
    "newFile",
    "recentDocs",
    "dataHistory",
    "edit",
    "undo",
    "redo",
    "cut",
    "copy",
    "paste",
    "pasteAndMatchStyle",
    "selectAll",
    "view",
    "goBack",
    "goForward",
    "globalSearch",
    "commandPanel",
    "zoomIn",
    "zoomOut",
    "actualSize",
    "toggleFullScreen",
    "window",
    "minimize",
    "zoom",
    "bringAllToFront",
    "help",
    "userGuide",
    "feedback",
    "debug",
];

const DEFAULT_LABELS = {
    appName: "QingYu",
    about: "About QingYu",
    config: "Settings",
    services: "Services",
    hide: "Hide QingYu",
    hideOthers: "Hide Others",
    showAll: "Show All",
    quit: "Quit QingYu",
    file: "File",
    newFile: "New Document",
    recentDocs: "Recent Documents",
    dataHistory: "Data History",
    edit: "Edit",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    pasteAndMatchStyle: "Paste and Match Style",
    selectAll: "Select All",
    view: "View",
    goBack: "Back",
    goForward: "Forward",
    globalSearch: "Global Search",
    commandPanel: "Command Palette",
    zoomIn: "Zoom In",
    zoomOut: "Zoom Out",
    actualSize: "Actual Size",
    toggleFullScreen: "Toggle Full Screen",
    window: "Window",
    minimize: "Minimize",
    zoom: "Zoom",
    bringAllToFront: "Bring All to Front",
    help: "Help",
    userGuide: "User Guide",
    feedback: "Feedback",
    debug: "Developer Tools",
};

const createDefaultNativeMenuState = () => ({
    ready: false,
    readonly: true,
    labels: {...DEFAULT_LABELS},
    accelerators: {},
});

const sanitizeNativeMenuState = (value) => {
    if (!value || typeof value !== "object" || typeof value.ready !== "boolean" ||
        typeof value.readonly !== "boolean" || !value.labels || typeof value.labels !== "object" ||
        !value.accelerators || typeof value.accelerators !== "object") {
        return;
    }

    const labels = {};
    for (const key of NATIVE_MENU_LABEL_KEYS) {
        const label = value.labels[key];
        if (typeof label !== "string" || label.length === 0 || label.length > 200) {
            return;
        }
        labels[key] = label;
    }

    const accelerators = {};
    NATIVE_MENU_COMMANDS.forEach((command) => {
        const accelerator = value.accelerators[command];
        if (typeof accelerator === "string" && accelerator.length <= 100) {
            accelerators[command] = accelerator;
        }
    });

    return {
        ready: value.ready,
        readonly: value.readonly,
        labels,
        accelerators,
    };
};

const createLegacyApplicationMenuTemplate = (productName) => [{
    label: productName,
    submenu: [{
        label: `About ${productName}`,
        role: "about",
    }, {type: "separator"}, {role: "services"}, {type: "separator"}, {
        label: `Hide ${productName}`,
        role: "hide",
    }, {role: "hideOthers"}, {role: "unhide"}, {type: "separator"}, {
        label: `Quit ${productName}`,
        role: "quit",
    }],
}, {
    role: "editMenu",
    submenu: [{role: "cut"}, {role: "copy"}, {role: "paste"}, {
        role: "pasteAndMatchStyle",
        accelerator: "CmdOrCtrl+Shift+C",
    }, {role: "selectAll"}],
}, {
    role: "windowMenu",
    submenu: [{role: "minimize"}, {role: "zoom"}, {role: "togglefullscreen"}, {type: "separator"},
        {role: "toggledevtools"}, {type: "separator"}, {role: "front"}],
}];

const createApplicationMenuTemplate = ({platform, productName, state, dispatch, hotKey2Electron}) => {
    if (platform !== "darwin") {
        return createLegacyApplicationMenuTemplate(productName);
    }

    const currentState = sanitizeNativeMenuState(state) || createDefaultNativeMenuState();
    const labels = currentState.labels;
    const businessEnabled = currentState.ready;
    const writeEnabled = businessEnabled && !currentState.readonly;
    const commandItem = (command, options = {}) => ({
        id: command,
        label: labels[command],
        accelerator: currentState.accelerators[command]
            ? hotKey2Electron(currentState.accelerators[command])
            : undefined,
        enabled: options.write ? writeEnabled : businessEnabled,
        click: () => dispatch(command),
    });

    return [{
        label: labels.appName,
        role: "appMenu",
        submenu: [{
            id: "about",
            label: labels.about,
            role: "about",
        }, {type: "separator"}, commandItem("config", {write: true}), {type: "separator"}, {
            id: "services",
            label: labels.services,
            role: "services",
        }, {type: "separator"}, {
            id: "hide",
            label: labels.hide,
            role: "hide",
        }, {
            id: "hideOthers",
            label: labels.hideOthers,
            role: "hideOthers",
        }, {
            id: "showAll",
            label: labels.showAll,
            role: "unhide",
        }, {type: "separator"}, {
            id: "quit",
            label: labels.quit,
            role: "quit",
        }],
    }, {
        label: labels.file,
        role: "fileMenu",
        submenu: [
            commandItem("newFile", {write: true}),
            commandItem("recentDocs", {write: true}),
            {type: "separator"},
            commandItem("dataHistory", {write: true}),
        ],
    }, {
        label: labels.edit,
        role: "editMenu",
        submenu: [{id: "undo", label: labels.undo, role: "undo"}, {
            id: "redo", label: labels.redo, role: "redo",
        }, {type: "separator"}, {id: "cut", label: labels.cut, role: "cut"}, {
            id: "copy", label: labels.copy, role: "copy",
        }, {id: "paste", label: labels.paste, role: "paste"}, {
            id: "pasteAndMatchStyle",
            label: labels.pasteAndMatchStyle,
            role: "pasteAndMatchStyle",
            accelerator: "CmdOrCtrl+Shift+C",
        }, {type: "separator"}, {
            ...commandItem("selectAll"),
            accelerator: "CmdOrCtrl+A",
        }],
    }, {
        label: labels.view,
        role: "viewMenu",
        submenu: [commandItem("goBack"), commandItem("goForward"), {type: "separator"},
            commandItem("globalSearch"), commandItem("commandPanel"), {type: "separator"}, {
                id: "zoomIn", label: labels.zoomIn, role: "zoomIn",
            }, {id: "zoomOut", label: labels.zoomOut, role: "zoomOut"}, {
                id: "actualSize", label: labels.actualSize, role: "resetZoom",
            }, {type: "separator"}, {
                id: "togglefullscreen", label: labels.toggleFullScreen, role: "togglefullscreen",
            }],
    }, {
        label: labels.window,
        role: "windowMenu",
        submenu: [{id: "minimize", label: labels.minimize, role: "minimize"}, {
            id: "zoom", label: labels.zoom, role: "zoom",
        }, {type: "separator"}, {id: "bringAllToFront", label: labels.bringAllToFront, role: "front"}],
    }, {
        label: labels.help,
        role: "help",
        submenu: [commandItem("userGuide", {write: true}), commandItem("feedback"), {type: "separator"},
            commandItem("debug")],
    }];
};

module.exports = {
    NATIVE_MENU_COMMANDS,
    NATIVE_MENU_LABEL_KEYS,
    createApplicationMenuTemplate,
    createDefaultNativeMenuState,
    sanitizeNativeMenuState,
};
