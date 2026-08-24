import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";
import {syncMarkdownFullscreenButton, syncMarkdownFullscreenModels} from "./fullscreenState";

let cleanup: () => void;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => cleanup());

const editorElement = () => {
    const element = document.createElement("div");
    element.innerHTML = '<button data-type="markdown-fullscreen"><svg><use xlink:href="#iconFullscreen"></use></svg></button>';
    document.body.append(element);
    return element;
};

test("keeps Markdown fullscreen mutually exclusive and synchronizes both button icons", () => {
    const first = editorElement();
    const second = editorElement();
    second.classList.add("fullscreen");
    syncMarkdownFullscreenModels([{element: first}, {element: second}], first, true);
    assert.equal(first.classList.contains("fullscreen"), true);
    assert.equal(second.classList.contains("fullscreen"), false);
    assert.equal(first.querySelector("use").getAttribute("xlink:href"), "#iconFullscreenExit");
    assert.equal(second.querySelector("use").getAttribute("xlink:href"), "#iconFullscreen");
});

test("synchronizes the current Markdown fullscreen button without desktop model discovery", () => {
    const element = editorElement();
    syncMarkdownFullscreenButton(element, true);
    assert.equal(element.querySelector("use").getAttribute("xlink:href"), "#iconFullscreenExit");
    syncMarkdownFullscreenButton(element, false);
    assert.equal(element.querySelector("use").getAttribute("xlink:href"), "#iconFullscreen");
});
