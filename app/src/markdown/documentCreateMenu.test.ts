import * as assert from "node:assert/strict";
import test from "node:test";
import {getDocumentCreateMenuItems} from "./documentCreateMenu";

test("offers only Markdown creation from an unencrypted document menu", () => {
    const app = {name: "test-app"};
    const calls: unknown[][] = [];
    const items = getDocumentCreateMenuItems({
        app,
        notebookId: "notebook-id",
        parentPath: "/parent",
        newFileLabel: "新建文档",
        encrypted: false,
        createMarkdown: async (...args) => {
            calls.push(["markdown", ...args]);
            return true;
        },
    });

    assert.deepEqual(items.map(({id, label, icon}) => ({id, label, icon})), [{
        id: "newMarkdown",
        label: "新建文档 Markdown",
        icon: "iconMarkdown",
    }]);

    items[0].click();
    assert.deepEqual(calls, [
        ["markdown", app, "notebook-id", "/parent"],
    ]);
});

test("offers no creation for an encrypted notebook", () => {
    const items = getDocumentCreateMenuItems({
        app: {},
        notebookId: "encrypted-id",
        parentPath: "/",
        newFileLabel: "新建文档",
        encrypted: true,
        createMarkdown: async () => true,
    });

    assert.deepEqual(items.map((item) => item.id), []);
});
