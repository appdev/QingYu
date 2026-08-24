const path = require("node:path");
const {isMarkdownFilePath} = require("./externalMarkdownService");

const unquote = (value) => value.length >= 2 && value[0] === '"' && value[value.length - 1] === '"'
    ? value.slice(1, -1)
    : value;

const extractExternalMarkdownPaths = (argv, {appIsPackaged, defaultApp, workingDirectory}) => {
    const start = appIsPackaged ? 1 : defaultApp ? 2 : 1;
    return argv.slice(start).flatMap((rawValue) => {
        const value = unquote(rawValue);
        if (!value || value.startsWith("--") || value.startsWith("qingyu://") || !isMarkdownFilePath(value)) return [];
        return [path.isAbsolute(value) ? path.normalize(value) : path.resolve(workingDirectory, value)];
    });
};

const redactExternalMarkdownArgs = (argv) => argv.map((rawValue) => {
    const value = unquote(rawValue);
    return value && !value.startsWith("--") && isMarkdownFilePath(value) ? "[external-markdown]" : rawValue;
});

const createExternalMarkdownWindowTabs = (descriptor) => [{
    title: descriptor.name,
    icon: "iconMarkdown",
    pin: false,
    active: true,
    instance: "Tab",
    action: "Tab",
    children: {
        instance: "MarkdownEditor",
        externalCapabilityId: descriptor.capabilityId,
    },
}];

const createExternalMarkdownOpenCoordinator = ({grant, findOwner, selectWindow, focusOwner, send, createWindow, onError}) => {
    const queue = [];
    const ready = new Set();
    let draining;

    const drain = () => {
        if (draining) return draining;
        draining = (async () => {
            while (queue.length > 0) {
                const entry = queue[0];
                try {
                    entry.descriptor ||= await grant(entry.filePath);
                    const owner = await findOwner(entry.descriptor.capabilityId);
                    if (owner !== undefined) {
                        queue.shift();
                        focusOwner(owner, entry.descriptor.capabilityId);
                    } else {
                        const payload = {status: "ok", descriptor: entry.descriptor};
                        const webContentsId = selectWindow([...ready]);
                        if (webContentsId !== undefined && ready.has(webContentsId)) {
                            queue.shift();
                            send(webContentsId, payload);
                        } else if (createWindow?.(payload)) {
                            queue.shift();
                        } else {
                            break;
                        }
                    }
                } catch (error) {
                    queue.shift();
                    const payload = {status: "error", code: error.code || "OPEN_FAILED"};
                    const webContentsId = selectWindow([...ready]);
                    if (webContentsId !== undefined && ready.has(webContentsId)) {
                        send(webContentsId, payload);
                    } else {
                        onError?.(payload);
                    }
                }
            }
        })().finally(() => {
            draining = undefined;
        });
        return draining;
    };

    return {
        enqueue(paths) {
            queue.push(...paths.map((filePath) => ({filePath})));
            void drain();
        },
        markReady(webContentsId) {
            ready.add(webContentsId);
            void drain();
        },
        removeWindow(webContentsId) {
            ready.delete(webContentsId);
        },
        drain,
    };
};

module.exports = {
    createExternalMarkdownWindowTabs,
    createExternalMarkdownOpenCoordinator,
    extractExternalMarkdownPaths,
    redactExternalMarkdownArgs,
};
