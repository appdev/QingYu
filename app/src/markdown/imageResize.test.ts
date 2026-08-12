import assert = require("node:assert/strict");
import {test} from "node:test";
import {persistedImageWidth} from "./markra-core/codemirror/image-resize";

test("keeps an explicit width when an image is dragged to the editor maximum", () => {
    assert.equal(persistedImageWidth(1200), 1200);
    assert.equal(persistedImageWidth(1196), 1196);
});
