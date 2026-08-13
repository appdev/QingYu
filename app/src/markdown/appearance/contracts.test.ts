import assert = require("node:assert/strict");
import test from "node:test";
import {
    appearanceVariableName,
    getAppearanceContract,
    listAppearanceContracts,
} from "./contracts";

const REQUIRED_CONTRACTS = [
    "shell.document",
    "shell.metadata",
    "shell.title",
    "editor.visual",
    "editor.source",
    "editor.cursor",
    "editor.selection",
    "editor.active-line",
    "editor.gutter",
    "editor.placeholder",
    "editor.scroller",
    "editor.drag-indicator",
    "editor.error",
    "block.paragraph",
    "block.heading-1",
    "block.heading-2",
    "block.heading-3",
    "block.heading-4",
    "block.heading-5",
    "block.heading-6",
    "block.list",
    "block.task",
    "block.blockquote",
    "block.callout",
    "block.horizontal-rule",
    "block.table",
    "block.code",
    "block.math",
    "block.mermaid",
    "block.raw-html",
    "inline.strong",
    "inline.emphasis",
    "inline.strikethrough",
    "inline.highlight",
    "inline.code",
    "inline.link",
    "inline.math",
    "media.image",
    "control.code-language",
    "control.code-actions",
    "control.table-toolbar",
    "control.fold",
    "control.block-toolbar",
    "control.math-macro",
    "overlay.code-language",
    "overlay.search",
    "overlay.footnote",
    "overlay.media-viewer",
    "state.syntax-hint",
    "state.trailing-space",
    "state.clipboard-progress",
] as const;

test("covers every approved editor appearance component exactly once", () => {
    const contracts = listAppearanceContracts();
    assert.deepEqual(contracts.map((contract) => contract.id).sort(), [...REQUIRED_CONTRACTS].sort());
    assert.equal(new Set(contracts.map((contract) => contract.id)).size, contracts.length);
    assert.deepEqual(new Set(contracts.map((contract) => contract.category)), new Set([
        "native-equivalent",
        "editor-foundation",
        "markdown-exclusive",
    ]));
});

test("assigns visible selectors to one contract and uses SiYuan fallbacks", () => {
    const contracts = listAppearanceContracts();
    const selectors = new Map<string, string>();
    for (const contract of contracts) {
        assert.ok(contract.markdownSelector);
        assert.ok(contract.reference.selector || contract.reference.variable);
        assert.ok(contract.states.length > 0);
        assert.ok(contract.platforms.length > 0);
        assert.ok(contract.modes.length > 0);
        assert.ok(contract.styleProperties.length > 0 || contract.geometry.length > 0);
        assert.ok(contract.fallbackVariables.every((name) => name.startsWith("--b3-")));
        for (const selector of [contract.markdownSelector, ...contract.ownedSelectors]) {
            assert.equal(selectors.get(selector), undefined, `${selector} is already owned by ${selectors.get(selector)}`);
            selectors.set(selector, contract.id);
        }
    }
});

test("provides stable lookup and component-scoped variable names", () => {
    assert.equal(getAppearanceContract("block.code")?.reference.selector, ".protyle-wysiwyg .code-block");
    assert.equal(
        appearanceVariableName("block.code", "backgroundColor"),
        "--b3-editor-appearance-block-code-background-color",
    );
    assert.equal(getAppearanceContract("missing"), undefined);
});
