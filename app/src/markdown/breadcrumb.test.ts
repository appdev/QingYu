import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {renderMarkdownBreadcrumb} from "./breadcrumb";

test("renders Markdown breadcrumb arrows between items", () => {
    const dom = new JSDOM(`<div class="protyle-breadcrumb__bar">${renderMarkdownBreadcrumb([
        "Linux",
        "folder",
        "未命名.md",
    ])}</div>`);
    const bar = dom.window.document.querySelector(".protyle-breadcrumb__bar");

    assert.deepEqual(Array.from(bar.children, (element) => element.getAttribute("class")), [
        "protyle-breadcrumb__item",
        "protyle-breadcrumb__arrow",
        "protyle-breadcrumb__item",
        "protyle-breadcrumb__arrow",
        "protyle-breadcrumb__item protyle-breadcrumb__item--active",
    ]);
    assert.equal(bar.querySelector(".protyle-breadcrumb__item .protyle-breadcrumb__arrow"), null);
    assert.equal(bar.lastElementChild.textContent.trim(), "未命名.md");
});

test("escapes Markdown breadcrumb names", () => {
    const dom = new JSDOM(`<div>${renderMarkdownBreadcrumb(["<Linux>", "notes&ideas.md"])}</div>`);

    assert.deepEqual(Array.from(dom.window.document.querySelectorAll(".protyle-breadcrumb__text"),
        (element) => element.textContent), ["<Linux>", "notes&ideas.md"]);
    assert.equal(dom.window.document.querySelector("script"), null);
});
