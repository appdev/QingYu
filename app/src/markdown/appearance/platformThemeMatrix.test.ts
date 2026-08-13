import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {listAppearanceContracts} from "./contracts";
import {applyThemeCss} from "./testSupport";
import {resolveMarkdownAppearanceForTest} from "./themeResolver";

export const STANDARD_THIRD_PARTY_THEME_CSS = `
:root { --b3-theme-on-background: rgb(10, 20, 30); }
.protyle-wysiwyg .code-block { border-radius: 13px; }
`;

let cleanup: () => void;
let removeTheme: (() => void) | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"code-block\" data-type=\"NodeCodeBlock\"><div class=\"protyle-action\"></div><div class=\"hljs\">code</div></div>",
            }),
        },
    });
});

afterEach(() => {
    removeTheme?.();
    removeTheme = undefined;
    cleanup();
});

test("standard third-party themes support variables and Protyle selector probes", () => {
    removeTheme = applyThemeCss(STANDARD_THIRD_PARTY_THEME_CSS);
    const snapshot = resolveMarkdownAppearanceForTest(document);

    assert.equal(snapshot.values["--b3-editor-appearance-shell-document-color"], "rgb(10, 20, 30)");
    assert.equal(snapshot.values["--b3-editor-appearance-block-code-border-radius"], "13px");
});

test("every appearance contract declares the shared desktop and mobile matrix", () => {
    for (const contract of listAppearanceContracts()) {
        assert.deepEqual(contract.platforms, ["desktop", "mobile"], contract.id);
    }
});

test("mobile differences stay in the shared responsive contract scope", () => {
    const source = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_markdown.scss"), "utf8");
    assert.match(source, /&\[data-markdown-platform="mobile"\]/u);
    assert.match(source, /@media \(hover: none\)/u);
    assert.doesNotMatch(source, /data-markdown-platform="mobile"[^}]*--b3-theme-/su);
});
