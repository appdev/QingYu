import assert = require("node:assert/strict");
import test from "node:test";
import {
    appearanceComparisonProperties,
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
    "block.callout-note",
    "block.callout-tip",
    "block.callout-important",
    "block.callout-warning",
    "block.callout-caution",
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
    "control.code-copy",
    "control.code-more",
    "control.table-toolbar",
    "control.table-button",
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

test("measures composite controls by equivalent responsibility", () => {
    assert.equal(getAppearanceContract("control.code-copy")?.markdownSelector, ".markra-code-copy-button");
    assert.equal(getAppearanceContract("control.code-more")?.markdownSelector, ".markra-code-more-button");
    assert.equal(getAppearanceContract("control.table-button")?.markdownSelector, ".markra-table-control");
});

test("probes every native callout subtype independently", () => {
    for (const type of ["note", "tip", "important", "warning", "caution"]) {
        const contract = getAppearanceContract(`block.callout-${type}`);
        assert.equal(
            contract?.reference.selector,
            `.protyle-wysiwyg [data-type="NodeCallout"][data-subtype="${type.toUpperCase()}"]`,
        );
        assert.equal(contract?.markdownSelector, `.cm-markra-callout[data-callout-type="${type}"]`);
    }
});

test("maps the native horizontal rule container and textured line independently", () => {
    const contract = getAppearanceContract("block.horizontal-rule");
    assert.equal(contract?.reference.selector, ".protyle-wysiwyg .hr > div");
    assert.deepEqual(contract?.propertyReferences?.background, {
        selector: ".protyle-wysiwyg .hr > div",
        property: "background",
        pseudo: "::after",
    });
    assert.deepEqual(contract?.propertyReferences?.backgroundColor, {
        selector: ".protyle-wysiwyg .hr > div",
        property: "backgroundColor",
        pseudo: "::after",
    });
    assert.deepEqual(contract?.propertyReferences?.lineHeight, {
        selector: ".protyle-wysiwyg .hr > div",
        property: "height",
        pseudo: "::after",
    });
    assert.deepEqual(contract?.propertyReferences?.lineTop, {
        selector: ".protyle-wysiwyg .hr > div",
        property: "top",
        pseudo: "::after",
    });
    assert.deepEqual(contract?.ownedSelectors, [".cm-markra-horizontal-rule__line"]);
});

test("compares heading spacing through CodeMirror-safe transparent borders", () => {
    for (let level = 1; level <= 6; level++) {
        const contract = getAppearanceContract(`block.heading-${level}`);
        assert.equal(contract?.markdownPropertyReferences?.marginTop.property, "borderTopWidth");
        assert.equal(contract?.markdownPropertyReferences?.marginBottom.property, "borderBottomWidth");
    }
});

test("compares paragraph and list spacing through CodeMirror-safe transparent borders", () => {
    for (const id of ["block.paragraph", "block.list"]) {
        const contract = getAppearanceContract(id);
        assert.equal(contract?.markdownPropertyReferences?.marginTop.property, "borderTopWidth");
        assert.equal(contract?.markdownPropertyReferences?.marginBottom.property, "borderBottomWidth");
    }
});

test("compares split code blocks through equivalent styles and aggregate geometry", () => {
    const contract = getAppearanceContract("block.code");
    assert.deepEqual(contract?.comparisonProperties, [
        "backgroundColor",
        "color",
        "fontFamily",
        "fontSize",
        "lineHeight",
    ]);
    assert.ok(contract?.styleProperties.includes("outerPaddingTop"));
    assert.equal(contract?.comparisonProperties.includes("outerPaddingTop"), false);
    assert.equal(contract?.comparisonProperties.includes("paddingTop"), false);
});

test("only compares native equivalents or explicitly declared component properties", () => {
    const paragraph = getAppearanceContract("block.paragraph");
    const source = getAppearanceContract("editor.source");
    const actions = getAppearanceContract("control.code-actions");
    assert.deepEqual(appearanceComparisonProperties(paragraph), paragraph?.styleProperties);
    assert.deepEqual(appearanceComparisonProperties(source), []);
    assert.deepEqual(appearanceComparisonProperties(actions), ["backgroundColor"]);
});
