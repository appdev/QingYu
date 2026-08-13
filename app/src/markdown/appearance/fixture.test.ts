import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {
    APPEARANCE_FIXTURE_MARKDOWN,
    createNativeAppearanceFixture,
} from "./fixture";

const {markdownToBlockDOM} = require("../../../scripts/markdownAppearanceFixture.cjs") as {
    markdownToBlockDOM(markdown: string): string;
};

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => cleanup());

test("uses the bundled Lute output as the complete native appearance fixture", () => {
    const root = createNativeAppearanceFixture(document, markdownToBlockDOM(APPEARANCE_FIXTURE_MARKDOWN));
    const code = root.querySelector<HTMLElement>(".code-block");
    assert.ok(code);
    assert.ok(code.querySelector(":scope > .protyle-action > .protyle-action__language"));
    assert.ok(code.querySelector(":scope > .protyle-action > .fn__flex-1"));
    assert.ok(code.querySelector(":scope > .protyle-action > .protyle-action__copy"));
    assert.ok(code.querySelector(":scope > .protyle-action > .protyle-action__menu"));
    assert.ok(code.querySelector(":scope > .hljs [contenteditable=true]"));
    assert.equal(root.dataset.appearanceFixture, "native");
});

test("covers representative native content and upgrades Markdown callouts to native probe nodes", () => {
    const blockDOM = markdownToBlockDOM(APPEARANCE_FIXTURE_MARKDOWN);
    const root = createNativeAppearanceFixture(document, blockDOM);
    const source = document.createElement("template");
    source.innerHTML = blockDOM;
    assert.ok(root.children.length >= source.content.children.length);
    assert.ok(root.querySelector(".h1"));
    assert.ok(root.querySelector(".list"));
    assert.ok(root.querySelector(".li[data-subtype='t'] > .protyle-action--task"));
    assert.equal(root.querySelector(".li[data-subtype='t'] .p .protyle-action--task"), null);
    assert.ok(root.querySelector(".bq"));
    assert.deepEqual(
        [...root.querySelectorAll<HTMLElement>('[data-type="NodeCallout"]')]
            .map((element) => element.dataset.subtype),
        ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"],
    );
    assert.ok(root.querySelector(".table"));
    assert.ok(root.querySelector(".hr"));
});
