import assert = require("node:assert/strict");
import test from "node:test";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";

const breadcrumbSource = readFileSync(resolve(process.cwd(), "src/protyle/breadcrumb/index.ts"), "utf8");
const constantsSource = readFileSync(resolve(process.cwd(), "src/constants.ts"), "utf8");
const dockUtilSource = readFileSync(resolve(process.cwd(), "src/layout/dock/util.ts"), "utf8");

test("native editor exposes a dedicated outline button that opens the local outline panel", () => {
    assert.match(breadcrumbSource, /data-type="outline"[\s\S]{0,200}iconOutline/u);
    assert.match(breadcrumbSource, /type === "outline"[\s\S]+openOutline\(\{/u);
});

test("native outline button does not relocate the existing left dock outline", () => {
    const leftDock = constantsSource.match(/left: \{[\s\S]+?right: \{/u)?.[0] || "";
    assert.match(leftDock, /type: "outline"/u);
});

test("native local outline opens after the editor on the right", () => {
    const openOutline = dockUtilSource.match(/export const openOutline[\s\S]+?export const resetFloatDockSize/u)?.[0] || "";
    assert.match(openOutline, /wnd\.split\("lr"\)/u);
    assert.doesNotMatch(openOutline, /wnd\.split\("lr", false\)/u);
});
