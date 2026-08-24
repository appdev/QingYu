import assert = require("node:assert/strict");
import test from "node:test";
import {applyMarkdownAdapterOverrides} from "./siyuanAdapterOverrides";

test("document source overrides replace only the selected adapter operations", () => {
    const links: string[] = [];
    const base = {
        openLink: (target: string) => links.push(`base:${target}`),
        resolveImageSource: (source: string) => `base:${source}`,
        saveClipboardAssets: async (): Promise<[]> => [],
    };
    const adapter = applyMarkdownAdapterOverrides(base, {
        openLink: (target: string) => links.push(target),
        resolveImageSource: (source: string) => `external:${source}`,
    });

    adapter.openLink("linked.md");

    assert.deepEqual(links, ["linked.md"]);
    assert.equal(adapter.resolveImageSource("image.png"), "external:image.png");
});
