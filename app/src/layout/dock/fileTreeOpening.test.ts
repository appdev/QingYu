import * as assert from "node:assert/strict";
import test from "node:test";
import {JSDOM} from "jsdom";
import {openFileTreeItem} from "./fileTreeOpening";

const createItem = () => new JSDOM("<li></li>").window.document.querySelector("li") as HTMLElement;

test("file tree opening lock is released after a successful open", async () => {
    const item = createItem();
    const opening = openFileTreeItem(item, async () => undefined);

    assert.equal(opening.started, true);
    assert.equal(item.getAttribute("data-opening"), "true");
    await opening.finished;
    assert.equal(item.hasAttribute("data-opening"), false);
});

test("file tree opening lock is released after an open failure", async () => {
    const item = createItem();
    const opening = openFileTreeItem(item, async () => {
        throw new Error("open failed");
    });

    await assert.rejects(opening.finished, /open failed/);
    assert.equal(item.hasAttribute("data-opening"), false);
});

test("file tree opening lock is released after a synchronous open failure", async () => {
    const item = createItem();
    const opening = openFileTreeItem(item, () => {
        throw new Error("open failed synchronously");
    });

    await assert.rejects(opening.finished, /open failed synchronously/);
    assert.equal(item.hasAttribute("data-opening"), false);
});

test("file tree opening lock prevents duplicate opens", async () => {
    const item = createItem();
    let release: () => void;
    const pending = new Promise<void>((resolve) => {
        release = resolve;
    });
    const first = openFileTreeItem(item, () => pending);
    const second = openFileTreeItem(item, async () => undefined);

    assert.equal(first.started, true);
    assert.equal(second.started, false);
    release();
    await first.finished;
});

test("a stale DOM opening marker does not block a new open", async () => {
    const item = createItem();
    item.setAttribute("data-opening", "true");

    const opening = openFileTreeItem(item, async () => undefined);

    assert.equal(opening.started, true);
    await opening.finished;
    assert.equal(item.hasAttribute("data-opening"), false);
});
