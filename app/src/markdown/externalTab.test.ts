import assert = require("node:assert/strict");
import test from "node:test";
import {collectExternalMarkdownCapabilityIds, markdownLayoutData} from "./externalTab";

test("external Markdown layout stores only the capability identifier", () => {
    assert.deepEqual(markdownLayoutData({kind: "external", capabilityId: "cap-1"}), {
        instance: "MarkdownEditor",
        externalCapabilityId: "cap-1",
    });
});

test("layout capability collection is recursive and deduplicated", () => {
    const layout = {
        children: [
            {instance: "MarkdownEditor", externalCapabilityId: "cap-1"},
            {children: [{instance: "MarkdownEditor", externalCapabilityId: "cap-1"}]},
            {instance: "MarkdownEditor", externalCapabilityId: "cap-2"},
        ],
    };

    assert.deepEqual(collectExternalMarkdownCapabilityIds(layout), ["cap-1", "cap-2"]);
});
