import assert = require("node:assert/strict");
import {test} from "node:test";
import {isMarkdownStatisticsOwnerEligible, StatusbarOwnership} from "./statusbarOwnership";
import {JSDOM} from "jsdom";

test("rejects stale commits after another model claims the status bar", () => {
    const ownership = new StatusbarOwnership();
    const markdown = {};
    const protyle = {};
    const markdownToken = ownership.claim(markdown);
    const protyleToken = ownership.claim(protyle);
    assert.equal(ownership.owns(markdown, markdownToken), false);
    assert.equal(ownership.owns(protyle, protyleToken), true);
});

test("releases only the current owner and invalidates its token", () => {
    const ownership = new StatusbarOwnership();
    const markdown = {};
    const token = ownership.claim(markdown);
    assert.equal(ownership.release({}), false);
    assert.equal(ownership.owns(markdown, token), true);
    assert.equal(ownership.release(markdown), true);
    assert.equal(ownership.owns(markdown, token), false);
});

test("allows statistics only for the focused Markdown editor in a visible tab", () => {
    const dom = new JSDOM('<div id="visible"><div id="editor"></div></div><div class="fn__none"><div id="hidden"></div></div>');
    const visible = dom.window.document.getElementById("editor");
    const hidden = dom.window.document.getElementById("hidden");
    assert.equal(isMarkdownStatisticsOwnerEligible({hasFocus: true}, visible), true);
    assert.equal(isMarkdownStatisticsOwnerEligible({hasFocus: false}, visible), false);
    assert.equal(isMarkdownStatisticsOwnerEligible({hasFocus: true}, hidden), false);
});
