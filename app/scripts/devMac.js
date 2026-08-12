const path = require("path");
const childProcess = require("child_process");

const createDevMacCommands = (projectRoot, electronPath, environment = process.env) => ({
    kernel: {
        command: "go",
        args: [
            "build",
            "-tags",
            "fts5 sqlcipher",
            "-o",
            path.join(projectRoot, "app/kernel/QingYu-Kernel"),
            ".",
        ],
        cwd: path.join(projectRoot, "kernel"),
    },
    electron: {
        command: electronPath,
        args: [path.join(projectRoot, "app/electron/main.js")],
        cwd: path.join(projectRoot, "app"),
        env: {
            ...environment,
            NODE_ENV: "development",
            QINGYU_DEV_MANAGED_KERNEL: "1",
        },
    },
});

const runCommand = ({command, args, cwd, env}) => new Promise((resolve, reject) => {
    const child = childProcess.spawn(command, args, {
        cwd,
        env,
        stdio: "inherit",
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
        if (code === 0) {
            resolve();
        } else {
            reject(new Error(`${command} exited with ${signal || code}`));
        }
    });
});

const prepareWebpackWatchConfig = (config) => ({
    ...config,
    watch: false,
});

const main = async () => {
    if (process.platform !== "darwin") {
        throw new Error("dev:mac is only available on macOS");
    }

    const projectRoot = path.resolve(__dirname, "../..");
    const commands = createDevMacCommands(projectRoot, require("electron"));
    console.log("[dev:mac] Building QingYu Kernel...");
    await runCommand(commands.kernel);

    const webpack = require("webpack");
    const createWebpackConfig = require("../webpack.config.js");
    const compiler = webpack(prepareWebpackWatchConfig(createWebpackConfig({}, {mode: "development"})));
    let electronProcess;
    let shuttingDown = false;
    let watcher;

    const shutdown = (code = 0) => {
        if (shuttingDown) {
            return;
        }
        shuttingDown = true;
        if (electronProcess && !electronProcess.killed) {
            electronProcess.kill("SIGTERM");
        }
        if (watcher) {
            watcher.close(() => process.exit(code));
        } else {
            process.exit(code);
        }
    };

    process.once("SIGINT", () => shutdown(0));
    process.once("SIGTERM", () => shutdown(0));

    console.log("[dev:mac] Watching desktop application bundle...");
    watcher = compiler.watch({}, (error, stats) => {
        if (error) {
            console.error(error);
            return;
        }
        console.log(stats.toString({colors: true, modules: false}));
        if (stats.hasErrors() || electronProcess) {
            return;
        }

        console.log("[dev:mac] Starting QingYu...");
        electronProcess = childProcess.spawn(commands.electron.command, commands.electron.args, {
            cwd: commands.electron.cwd,
            env: commands.electron.env,
            stdio: "inherit",
        });
        electronProcess.once("error", (spawnError) => {
            console.error(spawnError);
            shutdown(1);
        });
        electronProcess.once("exit", (code) => shutdown(code || 0));
    });
};

if (require.main === module) {
    main().catch((error) => {
        console.error(`[dev:mac] ${error.message}`);
        process.exit(1);
    });
}

module.exports = {
    createDevMacCommands,
    prepareWebpackWatchConfig,
};
