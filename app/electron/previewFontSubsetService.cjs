const path = require("node:path");
const {Worker} = require("node:worker_threads");

function createPreviewFontSubsetService({timeout = 5000, idle = 60000} = {}) {
    const clients = new Map();
    const stop = (owner) => {
        const client = clients.get(owner);
        if (!client) return;
        clients.delete(owner);
        clearTimeout(client.idleTimer);
        owner.removeListener("destroyed", client.destroyed);
        void client.worker.terminate();
        for (const {resolve, timer} of client.pending.values()) {
            clearTimeout(timer);
            resolve();
        }
        client.pending.clear();
        client.source = undefined;
    };
    return {
        stop,
        request(owner, data) {
            const {css, text} = data || {};
            if (owner.isDestroyed() || typeof css !== "string" || css.length > 64 * 1024 * 1024 ||
                typeof text !== "string" || text.length > 16384) return Promise.resolve();
            try {
                let client = clients.get(owner);
                if (client?.pending.size >= 4) return Promise.resolve();
                if (!client) {
                    const worker = new Worker(path.join(__dirname, "previewFontSubsetWorker.cjs"));
                    client = {worker, pending: new Map(), sourceID: 0, nextID: 0, destroyed: () => stop(owner)};
                    clients.set(owner, client);
                    owner.once("destroyed", client.destroyed);
                    const stopCurrent = () => { if (clients.get(owner) === client) stop(owner); };
                    worker.on("error", stopCurrent);
                    worker.on("exit", stopCurrent);
                    worker.on("message", ({id, css: result}) => {
                        const pending = client.pending.get(id);
                        if (!pending) return;
                        client.pending.delete(id);
                        clearTimeout(pending.timer);
                        pending.resolve(result);
                        if (client.pending.size === 0) {
                            client.idleTimer = setTimeout(stopCurrent, idle);
                            client.idleTimer.unref();
                        }
                    });
                }
                clearTimeout(client.idleTimer);
                const changed = client.source !== css;
                if (changed) {
                    client.source = css;
                    client.sourceID++;
                }
                const id = ++client.nextID;
                return new Promise((resolve) => {
                    const timer = setTimeout(() => stop(owner), timeout);
                    client.pending.set(id, {resolve, timer});
                    try {
                        client.worker.postMessage({id, sourceID: client.sourceID, css: changed ? css : undefined, text});
                    } catch {
                        stop(owner);
                    }
                });
            } catch {
                stop(owner);
                return Promise.resolve();
            }
        },
    };
}

module.exports = {createPreviewFontSubsetService};
