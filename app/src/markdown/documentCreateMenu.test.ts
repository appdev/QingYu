import * as assert from "node:assert/strict";
import test from "node:test";
import {getDocumentCreateMenuItems} from "./documentCreateMenu";

test("offers native and Markdown creation from an unencrypted document menu", () => {
    const app = {name: "test-app"};
    const calls: unknown[][] = [];
    const items = getDocumentCreateMenuItems({
        app,
        notebookId: "notebook-id",
        parentPath: "/parent",
        newFileLabel: "新建文档",
        encrypted: false,
        createNative: (...args) => calls.push(["native", ...args]),
        createMarkdown: async (...args) => {
            calls.push(["markdown", ...args]);
            return true;
        },
    });

    assert.deepEqual(items.map(({id, label, icon}) => ({id, label, icon})), [{
        id: "newDocument",
        label: "新建文档",
        icon: "iconAddDoc",
    }, {
        id: "newMarkdown",
        label: "新建文档 Markdown",
        icon: "iconMarkdown",
    }]);

    items[0].click();
    items[1].click();
    assert.deepEqual(calls, [
        ["native", app, "notebook-id", "/parent"],
        ["markdown", app, "notebook-id", "/parent"],
    ]);
});

test("keeps native creation available for an encrypted notebook", () => {
    const items = getDocumentCreateMenuItems({
        app: {},
        notebookId: "encrypted-id",
        parentPath: "/",
        newFileLabel: "新建文档",
        encrypted: true,
        createNative: () => undefined,
        createMarkdown: async () => true,
    });

    assert.deepEqual(items.map((item) => item.id), ["newDocument"]);
});
