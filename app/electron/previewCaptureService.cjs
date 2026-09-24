const {BrowserWindow} = require("electron");

async function prepareCapture(data) {
    const setAttributes = (element, attributes) => {
        for (const attribute of Array.from(element.attributes)) element.removeAttribute(attribute.name);
        for (const [name, value] of attributes) {
            if (!name.toLowerCase().startsWith("on")) element.setAttribute(name, value);
        }
    };
    setAttributes(document.documentElement, data.root);
    setAttributes(document.body, data.body);
    let base = document.querySelector("base");
    if (!base) {base = document.createElement("base"); document.head.append(base);}
    base.href = data.base;
    const signature = JSON.stringify(data.styles);
    if (window.captureStyles !== signature) {
        document.querySelectorAll("[data-capture-style]").forEach(element => element.remove());
        await Promise.all(data.styles.map(style => new Promise((resolve, reject) => {
            const element = document.createElement(style.href ? "link" : "style");
            element.dataset.captureStyle = "true";
            element.media = style.media || "";
            if (style.href) {
                element.rel = "stylesheet";
                element.href = style.href;
                element.onload = resolve;
                element.onerror = () => reject(new Error("Card stylesheet failed to load"));
            } else {
                element.textContent = style.css;
            }
            document.head.append(element);
            if (!style.href) resolve();
        })));
        window.captureStyles = signature;
    }
    const parsed = new DOMParser().parseFromString(data.html, "text/html");
    parsed.querySelectorAll("script, iframe, object, embed, base, meta, link, style").forEach(element => element.remove());
    for (const element of parsed.querySelectorAll("*")) {
        for (const attribute of Array.from(element.attributes)) {
            if (attribute.name.toLowerCase().startsWith("on")) element.removeAttribute(attribute.name);
        }
    }
    document.body.replaceChildren(...parsed.body.childNodes);
    document.body.style.margin = "0";
    const host = document.body.firstElementChild;
    if (!host?.classList.contains("notebook-root__capture")) throw new Error("Invalid card root");
    host.style.position = "fixed"; host.style.left = "0"; host.style.top = "0";
    await Promise.all(Array.from(host.querySelectorAll("img"), image => image.decode()));
    host.getBoundingClientRect();
    await document.fonts.ready;
    if (Array.from(document.fonts).some(font => font.status === "error")) throw new Error("Card font failed to load");
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
}

function createPreviewCaptureService() {
    const clients = new Map();
    const stop = owner => {
        const client = clients.get(owner);
        if (!client) return;
        clients.delete(owner);
        clearTimeout(client.idleTimer);
        owner.removeListener("destroyed", client.destroyed);
        owner.removeListener("render-process-gone", client.destroyed);
        if (!client.window.isDestroyed()) client.window.destroy();
    };
    return {
        stop,
        async request(event, data) {
            const owner = event.sender;
            const origin = new URL(owner.getURL());
            if (event.senderFrame !== owner.mainFrame || !["http:", "https:"].includes(origin.protocol) ||
                !data || typeof data.html !== "string" || data.html.length > 16 * 1024 * 1024 ||
                !Array.isArray(data.styles) || data.styles.length > 512 ||
                !Array.isArray(data.root) || !Array.isArray(data.body) ||
                new URL(data.base).origin !== origin.origin || JSON.stringify(data).length > 32 * 1024 * 1024) {
                throw new Error("Invalid card capture request");
            }
            let client = clients.get(owner);
            if (client?.busy) throw new Error("Card capture is busy");
            if (client && (client.origin !== origin.origin || client.window.isDestroyed())) {stop(owner); client = undefined;}
            if (!client) {
                const window = new BrowserWindow({show: false, width: 640, height: 960, webPreferences: {
                    session: owner.session, sandbox: true, contextIsolation: true, nodeIntegration: false,
                    backgroundThrottling: false,
                }});
                window.webContents.setWindowOpenHandler(() => ({action: "deny"}));
                window.webContents.on("will-navigate", event => event.preventDefault());
                client = {window, origin: origin.origin, destroyed: () => stop(owner)};
                clients.set(owner, client);
                owner.once("destroyed", client.destroyed);
                owner.once("render-process-gone", client.destroyed);
                client.ready = window.loadURL(new URL("/stage/card-preview.html", origin).href).then(async () => {
                    window.webContents.debugger.attach("1.3");
                    await window.webContents.debugger.sendCommand("Emulation.setDeviceMetricsOverride", {
                        width: 640, height: 960, deviceScaleFactor: 1, mobile: false,
                    });
                });
            }
            client.busy = true;
            clearTimeout(client.idleTimer);
            let timer;
            try {
                return await Promise.race([
                    (async () => {
                        await client.ready;
                        await client.window.webContents.executeJavaScript(`(${prepareCapture})(${JSON.stringify(data)})`);
                        const result = await client.window.webContents.debugger.sendCommand("Page.captureScreenshot", {
                            format: "png", optimizeForSpeed: true, captureBeyondViewport: false,
                            clip: {x: 0, y: 0, width: 640, height: 960, scale: 1},
                        });
                        const bytes = Buffer.from(result.data, "base64");
                        if (bytes.length > 3 * 1024 * 1024) throw new Error("Card PNG is too large");
                        return bytes;
                    })(),
                    new Promise((_, reject) => {timer = setTimeout(() => reject(new Error("Card capture timed out")), 10000);}),
                ]);
            } catch (error) {
                stop(owner);
                throw error;
            } finally {
                clearTimeout(timer);
                client.busy = false;
                if (clients.get(owner) === client) {
                    client.idleTimer = setTimeout(() => stop(owner), 60000);
                    client.idleTimer.unref();
                }
            }
        },
    };
}

module.exports = {createPreviewCaptureService};
