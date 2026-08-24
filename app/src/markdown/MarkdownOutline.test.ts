import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";

let cleanup: () => void;
let MarkdownOutlineView: typeof import("./markdownOutlineView").MarkdownOutlineView;
beforeEach(async () => {
    cleanup = installMarkdownTestDom();
    Object.assign(globalThis, {
        NODE_ENV: "test",
        SIYUAN_VERSION: "test",
    });
    Object.assign(window, {siyuan: {
        altIsPressed: false,
        ctrlIsPressed: false,
        shiftIsPressed: false,
        languages: {emptyContent: "Empty", expandAll: "Expand", filterKeywordEnter: "Filter", foldAll: "Collapse", outline: "Outline"},
    }});
    ({MarkdownOutlineView} = await import("./markdownOutlineView"));
});

afterEach(() => cleanup());

test("renders, filters, navigates, and live-updates a Markdown outline", () => {
    let focused = -1;
    const element = document.body.appendChild(document.createElement("div"));
    const outline = new MarkdownOutlineView(element, {filter: "Filter", outline: "Outline"},
        (position) => focused = position);
    assert.ok(element.classList.contains("sy__outline"));
    assert.ok(element.classList.contains("file-tree"));
    outline.update([{from: 3, level: 1, title: "First", to: 10}]);
    assert.equal(element.querySelector("[data-position='3'] .b3-list-item__text")?.textContent, "First");
    element.querySelector<HTMLElement>("[data-position='3']")?.click();
    assert.equal(focused, 3);
    outline.update([{from: 20, level: 2, title: "Second", to: 28}]);
    assert.equal(element.querySelector("[data-position='20'] .b3-list-item__text")?.textContent, "Second");
    const filter = element.querySelector<HTMLInputElement>("input");
    filter.value = "missing";
    filter.dispatchEvent(new Event("input", {bubbles: true}));
    assert.equal(element.querySelector(".b3-list-item[data-position]"), null);
    outline.destroy();
});

test("renders headings as an accessible nested tree", () => {
    const element = document.body.appendChild(document.createElement("div"));
    const outline = new MarkdownOutlineView(element, {filter: "Filter", outline: "Outline"}, () => undefined);
    outline.update([
        {from: 0, level: 1, title: "Parent", to: 8},
        {from: 10, level: 2, title: "Child", to: 18},
        {from: 20, level: 3, title: "Grandchild", to: 32},
    ]);
    const root = element.querySelector('[role="tree"]');
    assert.ok(root);
    assert.equal(root.querySelectorAll(':scope > [role="treeitem"]').length, 1);
    assert.equal(root.querySelectorAll('[role="group"]').length, 2);
    assert.equal(root.querySelector('[data-position="20"]')?.closest('[role="treeitem"]')?.getAttribute("aria-level"), "3");
    outline.destroy();
});
