import * as assert from "node:assert/strict";
import {describe, it} from "node:test";

const {createDevMacCommands, prepareWebpackWatchConfig} = require("../../scripts/devMac.js");

describe("macOS development launcher", () => {
    it("rebuilds the kernel and lets Electron choose the workspace", () => {
        const commands = createDevMacCommands("/project", "/electron");

        assert.deepEqual(commands.kernel, {
            command: "go",
            args: ["build", "-tags", "fts5 sqlcipher", "-o", "/project/app/kernel/QingYu-Kernel", "."],
            cwd: "/project/kernel",
        });
        assert.deepEqual(commands.electron.args, ["/project/app/electron/main.js"]);
        assert.equal(commands.electron.args.some((item: string) => item.startsWith("--workspace=")), false);
        assert.equal(commands.electron.env.NODE_ENV, "development");
        assert.equal(commands.electron.env.QINGYU_DEV_MANAGED_KERNEL, "1");
    });

    it("lets the launcher own the Webpack watch lifecycle", () => {
        const source = {mode: "development", watch: true};

        const prepared = prepareWebpackWatchConfig(source);

        assert.deepEqual(prepared, {mode: "development", watch: false});
        assert.equal(source.watch, true);
    });
});
