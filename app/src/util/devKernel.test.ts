import * as assert from "node:assert/strict";
import {describe, it} from "node:test";

const {shouldSpawnKernel} = require("../../electron/devKernel.js");

describe("managed development kernel", () => {
    it("spawns the first development kernel only when explicitly managed", () => {
        assert.equal(shouldSpawnKernel({development: true, managed: false, workspaceCount: 0}), false);
        assert.equal(shouldSpawnKernel({development: true, managed: true, workspaceCount: 0}), true);
        assert.equal(shouldSpawnKernel({development: true, managed: false, workspaceCount: 1}), true);
        assert.equal(shouldSpawnKernel({development: false, managed: false, workspaceCount: 0}), true);
    });
});
