import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {renderMarkdownBreadcrumb} from "./breadcrumb";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";

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

test("workspace Markdown breadcrumbs link the notebook root while external paths stay inert", () => {
    const workspace = new JSDOM(`<div>${renderMarkdownBreadcrumb(["工作笔记", "记录.md"], "box-id")}</div>`);
    const root = workspace.window.document.querySelector("[data-type='notebook-root']");
    assert.equal(root?.tagName, "BUTTON");
    assert.equal(root?.getAttribute("data-notebook-id"), "box-id");
    assert.equal(root?.textContent.trim(), "工作笔记");

    const external = new JSDOM(`<div>${renderMarkdownBreadcrumb(["外部 Markdown", "记录.md"])}</div>`);
    assert.equal(external.window.document.querySelector("[data-type='notebook-root']"), null);
});

test("native and Markdown editors route notebook breadcrumbs to the same notebook root action", () => {
    const nativeSource = readFileSync(resolve(process.cwd(), "src/protyle/breadcrumb/index.ts"), "utf8");
    const markdownSource = readFileSync(resolve(process.cwd(), "src/markdown/MarkdownEditor.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_markdown.scss"), "utf8");
    assert.match(nativeSource, /renderNotebookRootBreadcrumbItem\(/);
    assert.match(nativeSource, /openNotebookRoot\(protyle\.app, protyle\.notebookId/);
    assert.match(nativeSource, /response\.data\.slice\(1\)/);
    assert.match(markdownSource, /action\?\.dataset\.type === "notebook-root"/);
    assert.match(markdownSource, /this\.openNotebookRootAction\(this\.notebookId/);
    assert.match(styles, /&__breadcrumb \{[\s\S]*?box-shadow: none;/);
});
