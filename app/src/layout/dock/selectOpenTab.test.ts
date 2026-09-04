import assert = require("node:assert/strict");
import test from "node:test";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";

const dockUtilSource = readFileSync(resolve(process.cwd(), "src/layout/dock/util.ts"), "utf8");
const selectOpenTabSource = dockUtilSource.match(/export const selectOpenTab[\s\S]+?export const adjustDockPadding/u)?.[0] || "";

test("file tree locate supports workspace Markdown editors", () => {
    assert.match(selectOpenTabSource, /tab\?\.model instanceof MarkdownEditor/u);
    assert.match(selectOpenTabSource, /files\.selectItem\(tab\.model\.notebookId, tab\.model\.path\)/u);
});

test("file tree locate ignores external Markdown editors", () => {
    assert.match(selectOpenTabSource, /MarkdownEditor && !tab\.model\.externalCapabilityId/u);
});
