import * as assert from "node:assert/strict";
import {describe, it} from "node:test";

const {
    NATIVE_MENU_COMMANDS,
    NATIVE_MENU_LABEL_KEYS,
    createApplicationMenuTemplate,
    sanitizeNativeMenuState,
} = require("../../electron/nativeMenu.js");

const createState = (overrides: Record<string, unknown> = {}) => {
    const labels = Object.fromEntries(NATIVE_MENU_LABEL_KEYS.map((key: string) => [key, key]));
    labels.appName = "QingYu";
    return {
        ready: true,
        readonly: false,
        labels,
        accelerators: Object.fromEntries(Array.from(NATIVE_MENU_COMMANDS, (command: string) => [command, `⌘${command[0]}`])),
        ...overrides,
    };
};

const findItem = (items: Array<Record<string, any>>, id: string): Record<string, any> => {
    for (const item of items) {
        if (item.id === id) {
            return item;
        }
        if (Array.isArray(item.submenu)) {
            const found = findItem(item.submenu, id);
            if (found) {
                return found;
            }
        }
    }
};

describe("nativeMenu state", () => {
	 it("omits the removed daily note command and label", () => {
		assert.equal(NATIVE_MENU_COMMANDS.has("dailyNote"), false);
		assert.equal(NATIVE_MENU_LABEL_KEYS.includes("dailyNote"), false);
	});

    it("rejects malformed states and strips unknown fields", () => {
        assert.equal(sanitizeNativeMenuState({ready: "yes"}), undefined);
        assert.equal(sanitizeNativeMenuState(createState({
            labels: {
                ...createState().labels,
                file: "x".repeat(201),
            },
        })), undefined);

        const state = sanitizeNativeMenuState({
            ...createState(),
            unknown: "ignored",
            accelerators: {
                ...createState().accelerators,
                arbitraryCommand: "⌘X",
            },
        });
        assert.ok(state);
        assert.equal("unknown" in state, false);
        assert.equal("arbitraryCommand" in state.accelerators, false);
    });
});

describe("nativeMenu template", () => {
    it("builds a localized macOS menu and dispatches allowlisted commands", () => {
        const commands: string[] = [];
        const template = createApplicationMenuTemplate({
            platform: "darwin",
            productName: "QingYu",
            state: createState(),
            dispatch: (command: string) => commands.push(command),
            hotKey2Electron: (key: string) => `electron:${key}`,
        });

        assert.deepEqual(template.map((item: Record<string, any>) => item.label), [
            "QingYu", "file", "edit", "view", "window", "help",
        ]);
        assert.deepEqual(template.map((item: Record<string, any>) => item.role), [
            "appMenu", "fileMenu", "editMenu", "viewMenu", "windowMenu", "help",
        ]);
		findItem(template, "newFile").click();
        assert.deepEqual(commands, ["newFile"]);
        assert.equal(findItem(template, "newFile").accelerator, "electron:⌘n");
        assert.equal(findItem(template, "quit").role, "quit");
        assert.equal(findItem(template, "undo").role, "undo");
        assert.equal(findItem(template, "selectAll").role, undefined);
        assert.equal(findItem(template, "selectAll").accelerator, "CmdOrCtrl+A");
        findItem(template, "selectAll").click();
        assert.deepEqual(commands, ["newFile", "selectAll"]);
        assert.equal(findItem(template, "togglefullscreen").role, "togglefullscreen");
		assert.equal(findItem(template, "bringAllToFront").role, "front");
		assert.equal(findItem(template, "dailyNote"), undefined);
    });

    it("disables workspace-dependent commands until ready and write commands in readonly mode", () => {
        const unavailable = createApplicationMenuTemplate({
            platform: "darwin",
            productName: "QingYu",
            state: createState({ready: false}),
            dispatch: (command: string): void => {
                void command;
            },
            hotKey2Electron: (key: string) => key,
        });
        assert.equal(findItem(unavailable, "globalSearch").enabled, false);

        const readonly = createApplicationMenuTemplate({
            platform: "darwin",
            productName: "QingYu",
            state: createState({readonly: true}),
            dispatch: (command: string): void => {
                void command;
            },
            hotKey2Electron: (key: string) => key,
        });
		["config", "newFile", "recentDocs", "dataHistory", "userGuide"].forEach((id) => {
            assert.equal(findItem(readonly, id).enabled, false, id);
        });
        assert.equal(findItem(readonly, "feedback").enabled, true);
    });

    it("keeps the existing minimal application menu on non-macOS platforms", () => {
        for (const platform of ["linux", "win32"]) {
            const template = createApplicationMenuTemplate({
                platform,
                productName: "QingYu",
                state: createState(),
                dispatch: (command: string): void => {
                    void command;
                },
                hotKey2Electron: (key: string) => key,
            });
            assert.equal(template.length, 3);
            assert.equal(findItem(template, "newFile"), undefined);
            assert.equal(template[1].role, "editMenu");
            assert.equal(template[2].role, "windowMenu");
        }
    });
});
