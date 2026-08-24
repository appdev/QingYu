import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {renderMermaidToSvg} from "./mermaid";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    cleanup();
});

test("invalid Mermaid source rejects without leaking Mermaid's error SVG", async () => {
    await assert.rejects(() => renderMermaidToSvg("graph TD\nA -->"));

    assert.equal(document.body.textContent?.includes("Syntax error in text"), false);
    assert.equal(document.body.querySelector("svg"), null);
});
