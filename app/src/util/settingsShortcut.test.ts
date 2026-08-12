import * as assert from "node:assert/strict";
import {describe, it} from "node:test";

Object.assign(globalThis, {SIYUAN_VERSION: "test", NODE_ENV: "test"});

describe("settings shortcut", () => {
    it("uses the platform primary modifier and comma", async () => {
        const {Constants} = await import("../constants");
        assert.deepEqual(Constants.SIYUAN_KEYMAP.general.config, {
            default: "⌘,",
            custom: "⌘,",
        });
    });
});
