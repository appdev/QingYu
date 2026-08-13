import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markdown/markraTestDom";
import {mountCodeLanguageMenu} from "./codeLanguageMenu";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => cleanup());

test("shared code language menu is searchable and keyboard accessible", () => {
    const selected: string[] = [];
    const anchor = document.body.appendChild(document.createElement("button"));
    const handle = mountCodeLanguageMenu({
        anchor,
        container: document.body,
        currentLanguage: "java",
        languages: ["bash", "java", "javascript"],
        labels: {clear: "Clear", search: "Search"},
        onFilter: ({languages}) => languages,
        onSelect: (value) => selected.push(value),
        position: () => undefined,
    });
    const input = handle.element.querySelector<HTMLInputElement>("input");
    assert.ok(input);
    input.value = "java";
    input.dispatchEvent(new Event("input", {bubbles: true}));
    assert.deepEqual(
        Array.from(handle.element.querySelectorAll<HTMLElement>(".b3-list-item"), (item) => item.textContent),
        ["Clear", "java", "javascript"],
    );
    input.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter", bubbles: true}));
    assert.deepEqual(selected, ["java"]);
    assert.equal(handle.element.isConnected, false);
});

test("keeps valid languages when a plugin filter fails", () => {
    const handle = mountCodeLanguageMenu({
        anchor: document.body.appendChild(document.createElement("button")),
        container: document.body,
        currentLanguage: "",
        languages: ["java", "java", "", "bash"],
        labels: {clear: "Clear", search: "Search"},
        onFilter: () => {
            throw new Error("plugin failure");
        },
        onSelect: () => undefined,
        position: () => undefined,
    });
    assert.deepEqual(
        Array.from(handle.element.querySelectorAll<HTMLElement>(".b3-list-item"), (item) => item.dataset.id),
        ["clearLanguage", "bash", "java"],
    );
    handle.destroy();
    assert.equal(handle.element.isConnected, false);
});
