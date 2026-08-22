const {app, BrowserWindow} = require("electron");
const remote = require("@electron/remote/main");

const debugPort = process.env.QINGYU_MARKDOWN_DEBUG_PORT || "9222";
const kernelPort = process.env.QINGYU_MARKDOWN_KERNEL_PORT || "9806";
const showWindow = process.env.QINGYU_MARKDOWN_SHOW === "1";
app.commandLine.appendSwitch("remote-debugging-port", debugPort);
remote.initialize();

app.whenReady().then(async () => {
    const window = new BrowserWindow({
        height: 800,
        show: showWindow,
        webPreferences: {
            contextIsolation: false,
            nodeIntegration: true,
            webSecurity: false,
            webviewTag: true,
        },
        width: 1200,
    });
    remote.enable(window.webContents);
    window.webContents.userAgent = `QingYu/1.0.0 Electron ${window.webContents.userAgent}`;
    await window.loadURL(`http://127.0.0.1:${kernelPort}`);
    const state = await window.webContents.executeJavaScript(`({
        harness: Boolean(window.__siyuanMarkdownAppearanceTest),
        readyState: document.readyState,
        scripts: Array.from(document.scripts, (script) => script.src).filter(Boolean),
        siyuan: Boolean(window.siyuan),
    })`);
    console.log(JSON.stringify(state));
});
