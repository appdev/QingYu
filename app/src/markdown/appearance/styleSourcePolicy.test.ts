import assert = require("node:assert/strict");
import test from "node:test";
import ts = require("typescript");
import {listAppearanceContracts} from "./contracts";
import {
    collectVisibleMarkraSelectors,
    isSelectorCoveredByContract,
    readMarkdownAppearanceSources,
} from "./testSupport";

test("Markdown appearance has no independent palette or legacy bridge variables", () => {
    for (const file of readMarkdownAppearanceSources()) {
        assert.doesNotMatch(file.text, /--b3-markdown-/u, file.path);
        assert.doesNotMatch(file.text, /#[\da-f]{3,8}\b/iu, file.path);
        assert.doesNotMatch(file.text, /\brgba?\s*\(/iu, file.path);
        assert.doesNotMatch(file.text, /\b(?:Canvas|CanvasText|Highlight|HighlightText)\b/u, file.path);
    }
});

test("every visible Markra selector belongs to an appearance contract", () => {
    const contracts = listAppearanceContracts();
    assert.deepEqual(
        collectVisibleMarkraSelectors().filter((selector) => !isSelectorCoveredByContract(selector, contracts)),
        [],
    );
});

const forbiddenBaseThemeProperties = new Set([
    "background",
    "backgroundColor",
    "border",
    "borderColor",
    "borderRadius",
    "boxShadow",
    "color",
    "fontFamily",
    "fontSize",
    "fontStyle",
    "fontWeight",
    "height",
    "lineHeight",
    "margin",
    "marginBottom",
    "marginLeft",
    "marginRight",
    "marginTop",
    "maxHeight",
    "maxWidth",
    "minHeight",
    "minWidth",
    "opacity",
    "padding",
    "paddingBottom",
    "paddingLeft",
    "paddingRight",
    "paddingTop",
    "width",
]);

const baseThemeVisualAllowlist = new Set([
    "src/markdown/markra-core/codemirror/theme.ts|&[data-markra-composing=\"true\"] .cm-selectionBackground|backgroundColor",
]);

test("CodeMirror base themes contain no independent visual declarations", () => {
    const violations: string[] = [];
    for (const file of readMarkdownAppearanceSources().filter((item) => item.path.endsWith(".ts"))) {
        const source = ts.createSourceFile(file.path, file.text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
        const visit = (node: ts.Node) => {
            if (
                ts.isCallExpression(node) &&
                ts.isPropertyAccessExpression(node.expression) &&
                node.expression.name.text === "baseTheme" &&
                ts.isObjectLiteralExpression(node.arguments[0])
            ) {
                for (const selectorProperty of node.arguments[0].properties) {
                    if (!ts.isPropertyAssignment(selectorProperty) || !ts.isObjectLiteralExpression(selectorProperty.initializer)) {
                        continue;
                    }
                    const selector = ts.isStringLiteral(selectorProperty.name) ? selectorProperty.name.text : selectorProperty.name.getText(source);
                    for (const declaration of selectorProperty.initializer.properties) {
                        if (!ts.isPropertyAssignment(declaration)) continue;
                        const property = declaration.name.getText(source).replaceAll(/["']/gu, "");
                        const key = `${file.path}|${selector}|${property}`;
                        if (forbiddenBaseThemeProperties.has(property) && !baseThemeVisualAllowlist.has(key)) {
                            violations.push(key);
                        }
                    }
                }
            }
            ts.forEachChild(node, visit);
        };
        visit(source);
    }
    assert.deepEqual(violations, []);
});
