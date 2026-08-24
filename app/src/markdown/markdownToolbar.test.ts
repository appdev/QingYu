import assert = require("node:assert/strict");
import {test} from "node:test";
import {JSDOM} from "jsdom";
import {createMarkdownMoreMenuItems, syncMarkdownModeToggle, type MarkdownMoreCommand} from "./markdownToolbar";

test("mode toggle presents the mode entered by the next click", () => {
    const dom = new JSDOM('<div id="editor"><button data-type="markdown-mode"><svg><use></use></svg></button></div>');
    const element = dom.window.document.getElementById("editor") as HTMLElement;
    const button = element.querySelector<HTMLElement>('[data-type="markdown-mode"]');
    const icon = button.querySelector("use");

    syncMarkdownModeToggle(element, true, {markdown: "Markdown", wysiwyg: "WYSIWYG"});
    assert.equal(button.getAttribute("aria-label"), "Markdown");
    assert.equal(icon.getAttribute("xlink:href"), "#iconEdit");

    syncMarkdownModeToggle(element, false, {markdown: "Markdown", wysiwyg: "WYSIWYG"});
    assert.equal(button.getAttribute("aria-label"), "WYSIWYG");
    assert.equal(icon.getAttribute("xlink:href"), "#iconPreview");
});

test("more menu exposes checked editor preferences through existing commands", () => {
    const commands: MarkdownMoreCommand[] = [];
    const items = createMarkdownMoreMenuItems(
        {justify: true, rtl: false, typewriterMode: true},
        {justify: "Justify", rtl: "RTL", typewriterMode: "Typewriter"},
        (command) => commands.push(command),
    );

    assert.deepEqual(items.map((item) => [item.id, item.icon, item.checked]), [
        ["markdownTypewriter", "iconFocus", true],
        ["markdownJustify", "iconAlignJustify", true],
        ["markdownRTL", "iconRtl", false],
    ]);
    items.forEach((item) => item.click?.(undefined as never, undefined as never));
    assert.deepEqual(commands, ["toggle-typewriter", "toggle-justify", "toggle-rtl"]);
});
