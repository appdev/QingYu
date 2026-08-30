const path = require("path");
const fs = require("fs");
const os = require("os");
const assert = require("node:assert/strict");
require("tsx/cjs");
const {app, BrowserWindow, ipcMain} = require("electron");
const sass = require("sass");
const ts = require("typescript");
const {JSDOM} = require("jsdom");
const {markdownToBlockDOM} = require("./markdownAppearanceFixture.cjs");
const {
    classifyMarkdownDrop,
    markdownFileTreeDragAttributes,
    orderedFileTreePaths,
} = require("../src/markdown/documentManagement.ts");
const {renderDeletedMarkdownList} = require("../src/markdown/deletedDocuments.ts");
const {restoreRecentlyClosedTab} = require("../src/markdown/recentDocuments.ts");
const {
    createMarkdownManagementCoordinator,
    shouldUnregisterMarkdownRendererNavigation,
} = require("../electron/markdownManagementCoordinator.js");

app.commandLine.appendSwitch("disable-gpu");

const testTwoRendererMarkdownManagementIPC = async () => {
    const registerChannel = "siyuan-markdown-management-register";
    const invokeChannel = "siyuan-markdown-management-invoke";
    const phaseChannel = "siyuan-markdown-management-prepare";
    const ackChannel = "siyuan-markdown-management-ack";
    const readyChannel = "siyuan-markdown-management-ready";
    const coordinator = createMarkdownManagementCoordinator({timeout: 1000});
    const coordinatorFixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "siyuan-markdown-coordinator-"));
    const productionCoordinatorPath = path.join(coordinatorFixtureDirectory, "managementCoordinator.js");
    ["documentManagement", "managementCoordinator"].forEach((moduleName) => {
        const source = fs.readFileSync(path.join(__dirname, `../src/markdown/${moduleName}.ts`), "utf8");
        const compiled = ts.transpileModule(source, {compilerOptions: {
            module: ts.ModuleKind.CommonJS,
            target: ts.ScriptTarget.ES2020,
        }}).outputText;
        fs.writeFileSync(path.join(coordinatorFixtureDirectory, `${moduleName}.js`), compiled);
    });
    const ready = new Set();
    let resolveReady;
    const allReady = new Promise((resolve) => { resolveReady = resolve; });
    ipcMain.on(registerChannel, (event) => {
        const workspace = new URL(event.sender.getURL()).origin;
        assert.equal(workspace, "null");
        ready.add(event.sender.id);
        coordinator.register(event.sender.id, workspace, (payload) => event.sender.send(phaseChannel, payload));
        if (ready.size === 2) resolveReady();
    });
    ipcMain.on(ackChannel, (event, payload) => {
        const workspace = new URL(event.sender.getURL()).origin;
        coordinator.ack(event.sender.id, workspace, payload);
    });
    ipcMain.handle(invokeChannel, (event, payload) => {
        assert.equal(payload.workspace, new URL(event.sender.getURL()).origin);
        return payload.action === "prepare"
            ? coordinator.prepare(event.sender.id, payload)
            : coordinator.commit(event.sender.id, payload);
    });
    const rendererHTML = (id) => `<!doctype html><meta http-equiv="Content-Security-Policy"
        content="default-src 'none'; script-src 'unsafe-inline'"><script>
        const {ipcRenderer} = require("electron");
        const {installMarkdownManagementRendererCoordinator} = require(${JSON.stringify(productionCoordinatorPath)});
        window.managementPhases = [];
        const editor = {
            managementID: "fixture-editor-${id}", notebookId: "box", path: "/dirty.md", revision: "revision-shared",
            flush: async () => { window.managementPhases.push("flush"); return true; },
            getRevision: () => editor.revision,
            applyWorkspaceDocumentReference: (notebook, nextPath, revision) => {
                editor.notebookId = notebook;
                editor.path = nextPath;
                editor.revision = revision;
                window.managementPhases.push("commit");
            },
        };
        installMarkdownManagementRendererCoordinator(ipcRenderer, window.location.origin, () => [editor]);
        window.invokeManagement = (payload) => ipcRenderer.invoke("${invokeChannel}", payload);
        window.managementEditorPath = () => editor.path;
    </script>`;
    const windows = [1, 2].map(() => new BrowserWindow({show: false, webPreferences: {
        nodeIntegration: true,
        contextIsolation: false,
    }}));
    try {
        windows.forEach((renderer) => {
            renderer.webContents.on("did-start-navigation", (_event, _url, isInPlace, isMainFrame) => {
                if (shouldUnregisterMarkdownRendererNavigation(isInPlace, isMainFrame)) {
                    coordinator.unregister(renderer.webContents.id);
                }
            });
            renderer.webContents.on("did-finish-load", () => renderer.webContents.send(readyChannel));
        });
        await Promise.all(windows.map((renderer, index) => renderer.loadURL(
            `data:text/html;charset=utf-8,${encodeURIComponent(rendererHTML(index + 1))}`,
        )));
        await allReady;
        await new Promise((resolve) => {
            windows[1].webContents.once("did-finish-load", resolve);
            windows[1].reload();
        });
        const ref = {kind: "markdown", notebook: "box", path: "/dirty.md"};
        const prepared = await windows[0].webContents.executeJavaScript(`window.invokeManagement(${JSON.stringify({
            action: "prepare", workspace: "null", operationID: "fixture-operation",
            ref, mode: "flush", expectedRevision: "revision-shared", excludedEditorID: "fixture-editor-1",
        })})`);
        assert.equal(prepared.ok, true);
        const committed = await windows[0].webContents.executeJavaScript(`window.invokeManagement(${JSON.stringify({
            action: "commit", workspace: "null", operationID: "fixture-operation",
            lease: "__LEASE__", mutation: {kind: "rename", from: ref,
                to: {kind: "markdown", notebook: "box", path: "/renamed.md"}, revision: "revision-next"},
        }).replace("__LEASE__", prepared.lease)})`);
        assert.equal(committed.ok, true);
        assert.deepEqual(await Promise.all(windows.map((renderer) => renderer.webContents.executeJavaScript(
            "window.managementPhases",
        ))), [["commit"], ["flush", "commit"]]);
        assert.deepEqual(await Promise.all(windows.map((renderer) => renderer.webContents.executeJavaScript(
            "window.managementEditorPath()",
        ))), ["/renamed.md", "/renamed.md"]);
    } finally {
        windows.forEach((renderer) => {
            coordinator.unregister(renderer.webContents.id);
            renderer.destroy();
        });
        ipcMain.removeAllListeners(registerChannel);
        ipcMain.removeAllListeners(ackChannel);
        ipcMain.removeHandler(invokeChannel);
        fs.rmSync(coordinatorFixtureDirectory, {recursive: true});
    }
};

app.whenReady().then(async () => {
    const css = sass.compile(path.join(__dirname, "../src/assets/scss/base.scss")).css;
    const nativeCodeBlockDOM = markdownToBlockDOM("```javascript\nconst nativeValue = 1;\nreturn nativeValue;\n```")
        .replace("const nativeValue = 1;", "<span class=\"hljs-keyword\">const</span>&nbsp;nativeValue = 1;");
    const nativeParagraphDOM = markdownToBlockDOM("First paragraph\n\nSecond paragraph");
    const window = new BrowserWindow({
        width: 500,
        show: false,
        webPreferences: {
            sandbox: true,
        },
    });
    await testTwoRendererMarkdownManagementIPC();
    const columns = Array.from({length: 8}, (_, index) => `<th>Column ${index + 1} with a long heading</th>`).join("");
    const cells = Array.from({length: 8}, (_, index) => `<td>Value ${index + 1} with content that keeps the table wide</td>`).join("");
    const longDocumentLines = Array.from({length: 80}, (_, index) => `<div class="cm-line">Paragraph ${index + 1}</div>`).join("");
    const managementDOM = new JSDOM('<section data-fixture="markdown-history" style="display:block;width:320px"><ul></ul></section>');
    renderDeletedMarkdownList(managementDOM.window.document.querySelector("ul"), [{
        id: "deleted-1", notebook: "box", originalPath: "/deleted.md", historyPath: "history/deleted.md",
        deletedAt: 1, size: 1, revision: "revision",
    }], {readonly: false, emptyText: "Empty", restoreText: "Restore", purgeText: "Purge"});
    const orderedManagementPaths = orderedFileTreePaths([
        {kind: "native", path: "/20260820000000-native.sy"},
        {kind: "markdown", path: "/notes.md"},
    ]);
    const managementTreeHTML = `<ul data-fixture="markdown-management-tree" data-sort-mode="custom" style="width:320px">
        <li data-kind="native" data-path="${orderedManagementPaths[0]}">Native</li>
        <li data-kind="markdown" ${markdownFileTreeDragAttributes()} data-path="${orderedManagementPaths[1]}">Markdown</li>
    </ul>`;
    const html = `<!doctype html><style>.syntax-token{color:rgb(224, 108, 117)}.hljs-keyword{color:rgb(197, 80, 90)}.toolbar-fixture .markra-table-control{opacity:1;pointer-events:auto}.parity-fixture .protyle-wysiwyg .code-block{background-color:rgb(24, 25, 26);border-radius:8px}${css}.third-party-theme .protyle-wysiwyg span[data-type~="code"]{color:rgb(235,87,87)}.third-party-theme .protyle-wysiwyg span[data-type~="mark"]{color:rgb(110,60,170)}</style>
<div class="protyle markdown-editor" style="--b3-theme-primary:rgb(66, 133, 244);height:600px;width:480px">
    <div class="protyle-content markdown-editor__content">
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography">
                <div class="cm-markra-table-wrap markra-table-controls-wrapper">
                    <div class="markra-table-scroll"><table class="cm-markra-table"><thead><tr>${columns}</tr></thead><tbody><tr>${cells}</tr></tbody></table></div>
                </div>
                <div class="cm-markra-table-wrap markra-table-controls-wrapper" data-width-mode="auto" id="table-width-mode-fixture">
                    <div class="markra-table-scroll"><table class="cm-markra-table" data-width-mode="auto"><thead><tr><th>短</th><th>较长的表格内容</th></tr></thead><tbody><tr><td>A</td><td>用于验证列宽模式</td></tr></tbody></table></div>
                </div>
                <span class="img markra-image-node markra-image-node-selected"><span class="markra-image-frame"><img alt="test" style="height:80px;width:160px"><span class="protyle-action__drag" style="display:block"></span></span></span>
                <br><span class="img markra-image-node"><span class="markra-image-frame" id="default-image-frame" style="width:506px"><img alt="default-size" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='506' height='538'%3E%3Crect width='506' height='538' fill='blue'/%3E%3C/svg%3E"><span class="protyle-action__drag" style="display:block"></span></span></span>
            </div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="visual-editor" style="--b3-border-color:rgb(210, 210, 210);--b3-border-radius:4px;--b3-dialog-shadow:0 4px 12px rgb(0 0 0 / 20%);--b3-editor-appearance-block-blockquote-padding-left:10px;--b3-editor-appearance-block-callout-note-border-radius:11px;--b3-editor-appearance-block-callout-note-box-shadow:inset 0 0 0 2px rgb(22, 92, 152);--b3-editor-appearance-block-callout-note-header-color:rgb(18, 88, 148);--b3-editor-appearance-block-heading-1-color:rgb(120, 40, 160);--b3-editor-appearance-block-heading-1-font-size:36px;--b3-editor-appearance-block-heading-1-margin-bottom:3px;--b3-editor-appearance-block-heading-1-margin-top:14px;--b3-editor-appearance-block-heading-1-padding-bottom:7px;--b3-editor-appearance-block-heading-1-padding-top:6px;--b3-editor-appearance-block-list-padding-left:0px;--b3-editor-appearance-inline-link-color:rgb(20, 110, 180);--b3-editor-appearance-shell-document-color:rgb(33, 33, 33);--b3-list-hover:rgb(235, 240, 250);--b3-theme-background:rgb(255, 255, 255);--b3-theme-on-background:rgb(33, 33, 33);--b3-theme-on-surface:rgb(90, 90, 90);--b3-theme-primary:rgb(66, 133, 244);height:200px;width:480px">
    <div class="cm-editor" data-markdown-mode="visual">
        <div class="cm-content">
            <div class="cm-line cm-markra-h1"><span class="syntax-token">Visual heading</span></div>
            <div class="cm-line cm-markra-h6 markra-heading-editing" id="heading-level-line"><span class="markra-heading-level-control"><button class="markra-heading-level-button" data-heading-level="H6">H6</button><span class="markra-heading-level-list"><button class="markra-heading-level-option">段落</button><button class="markra-heading-level-option">H1</button><button class="markra-heading-level-option">H2</button><button class="markra-heading-level-option">H3</button><button class="markra-heading-level-option">H4</button><button class="markra-heading-level-option">H5</button><button class="markra-heading-level-option">H6</button></span></span><span>###### 目标</span></div>
            <div class="cm-line"><span class="syntax-token cm-markra-link">Visual link</span></div>
            <div class="cm-line cm-markra-list-item"><span>List item</span></div>
            <div class="cm-line cm-markra-blockquote cm-markra-blockquote-first cm-markra-blockquote-last"><span>Quote</span></div>
            <div class="cm-line cm-markra-callout markra-callout markra-callout-note markra-callout-first markra-callout-last" data-callout-type="note"><span class="markra-callout-header"><span class="markra-callout-title">Note</span></span></div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="textured-heading-editor" style="--b3-editor-appearance-block-heading-1-background:linear-gradient(90deg,rgb(160,120,20),rgb(250,220,120));--b3-editor-appearance-block-heading-1-background-blend-mode:multiply;--b3-editor-appearance-block-heading-1-background-clip:text;--b3-editor-appearance-block-heading-1-color:transparent;--b3-editor-appearance-block-heading-1-webkit-text-fill-color:transparent;--b3-editor-appearance-block-heading-1-caret-color:rgb(160,120,20);width:480px">
    <div class="cm-editor" data-markdown-mode="visual"><div class="cm-content"><div class="cm-line cm-markra-h1">Textured heading</div></div></div>
</div>
<div class="protyle markdown-editor third-party-theme" id="inline-theme-editor" style="--b3-editor-appearance-inline-code-color:rgb(235,87,87);--b3-editor-appearance-inline-highlight-color:rgb(110,60,170);--b3-theme-on-background:rgb(33,33,33);width:480px">
    <div class="protyle-wysiwyg"><div class="p"><div contenteditable="true"><span data-type="code">native code</span><span data-type="mark">native mark</span></div></div></div>
    <div class="cm-editor" data-markdown-mode="visual"><div class="cm-content"><div class="cm-line cm-markra-paragraph"><span class="plain-inline-text">plain</span><span class="cm-markra-inline-code">markdown code</span><span class="cm-markra-highlight">markdown mark</span></div></div></div>
</div>
<div class="protyle markdown-editor" id="paragraph-spacing-editor" style="--b3-font-size-editor:16px;width:480px">
    <div class="protyle-wysiwyg" data-appearance-fixture="native">${nativeParagraphDOM}</div>
    <div class="cm-editor" data-markdown-mode="visual">
        <div class="cm-content">
            <div class="cm-line cm-markra-paragraph p"><span>First paragraph</span></div>
            <div class="cm-line cm-markra-empty-line"><br></div>
            <div class="cm-line cm-markra-paragraph p"><span>Second paragraph</span></div>
            <div class="cm-line cm-markra-empty-line cm-markra-active-empty-line" id="active-empty-line"><br></div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="large-image-editor" style="width:1000px">
    <div class="markdown-editor__surface b3-typography">
        <span class="img markra-image-node"><span class="markra-image-frame markra-image-frame-sized" style="width:800px"><img class="cm-markra-image" alt="enlarged" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='506' height='538'%3E%3Crect width='506' height='538' fill='blue'/%3E%3C/svg%3E"><span class="protyle-action__drag" style="display:block"></span></span></span>
    </div>
</div>
<div class="protyle markdown-editor" id="empty-editor" style="height:600px;width:480px">
    <div class="protyle-content markdown-editor__content">
        <div class="protyle-top markdown-editor__top">
            <div class="protyle-title markdown-editor__title">Untitled.md</div>
        </div>
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography"></div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="document-scroll-editor" style="height:320px;width:480px">
    <div class="protyle-content markdown-editor__content">
        <div class="protyle-top markdown-editor__top">
            <div class="protyle-background markdown-editor__metadata protyle-background--enable"></div>
            <div class="protyle-title markdown-editor__title"><div class="protyle-title__input">Long document</div></div>
        </div>
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography">
                <div class="cm-editor" data-markdown-mode="visual">
                    <div class="cm-scroller"><div class="cm-content">${longDocumentLines}</div></div>
                </div>
            </div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="header-editor" style="--b3-font-family-protyle:serif;--b3-font-size-editor:18px;--b3-theme-background:rgb(30, 30, 30);--b3-theme-on-background:rgb(235, 235, 235);--b3-theme-on-surface-light:rgb(150, 150, 150);width:480px">
    <div class="protyle-top markdown-editor__top">
        <div class="protyle-background markdown-editor__metadata protyle-background--enable">
            <div class="protyle-background__img markdown-editor__cover"><img alt="cover"></div>
            <div class="protyle-background__ia"><div class="protyle-background__icon fn__none"></div><div class="b3-chips b3-chips__doctag markdown-editor__tags"><span class="b3-chip b3-chip--middle">tag</span></div><div class="protyle-background__action markdown-editor__actions"><button class="b3-button b3-button--cancel" data-type="tag"><svg></svg>Add Tag</button><button class="b3-button b3-button--cancel" data-type="icon"><svg></svg>Add icon</button><button class="b3-button b3-button--cancel" data-type="cover"><svg></svg>Add cover</button></div></div>
        </div>
        <div class="protyle-title markdown-editor__title"><div class="protyle-title__input">Theme title</div></div>
    </div>
</div>
<div class="protyle markdown-editor" data-markdown-platform="mobile" id="mobile-editor" style="height:640px;width:375px">
    <div class="protyle-content markdown-editor__content">
        <div class="protyle-top markdown-editor__top">
            <div class="protyle-title markdown-editor__title">Mobile.md</div>
        </div>
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography">
                <div class="cm-markra-table-wrap markra-table-controls-wrapper">
                    <div class="markra-table-scroll"><table class="cm-markra-table"><thead><tr>${columns}</tr></thead><tbody><tr>${cells}</tr></tbody></table></div>
                </div>
            </div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor toolbar-fixture" id="toolbar-editor" style="--b3-border-color:rgb(80, 80, 80);--b3-editor-appearance-control-table-button-height:24px;--b3-editor-appearance-control-table-button-width:24px;--b3-list-hover:rgb(55, 55, 55);--b3-theme-background:rgb(30, 30, 30);--b3-theme-error:rgb(240, 80, 80);--b3-theme-on-surface:rgb(180, 180, 180);--b3-theme-primary:rgb(66, 133, 244);width:320px">
    <div class="markdown-editor__surface b3-typography">
        <div class="cm-markra-table-wrap markra-table-controls-wrapper">
            <span class="markra-table-align-controls">
                <span class="markra-table-control-group markra-table-size-controls" data-table-control-group="size"><button class="markra-table-control markra-table-size-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-alignment-controls" data-table-control-group="alignment"><button class="markra-table-control markra-table-align-button" aria-pressed="true"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-width-controls" data-table-control-group="width"><button class="markra-table-control markra-table-width-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-delete-controls" data-table-control-group="delete"><button class="markra-table-control markra-table-delete-table"><svg class="markra-table-control-icon"></svg></button></span>
            </span>
            <div class="markra-table-scroll"><table class="cm-markra-table"><tbody><tr><td>内容</td></tr></tbody></table></div>
    </div>
</div>
</div>
<div class="protyle markdown-editor parity-fixture" id="code-parity-editor" style="--b3-font-family-code:'Theme Code';--b3-editor-appearance-block-code-background-color:rgb(24, 25, 26);--b3-editor-appearance-block-code-border-radius:8px;--b3-editor-appearance-control-code-copy-border-radius:8px 0 0 8px;--b3-editor-appearance-control-code-copy-color:rgb(41, 42, 43);--b3-editor-appearance-control-code-copy-opacity:0;--b3-editor-appearance-control-code-language-border-radius:3px;--b3-editor-appearance-control-code-language-color:rgb(91, 92, 93);--b3-editor-appearance-control-code-language-padding-left:8px;--b3-editor-appearance-control-code-language-padding-right:8px;--b3-editor-appearance-control-code-more-border-radius:0 8px 8px 0;--b3-editor-appearance-control-code-more-color:rgb(41, 42, 43);--b3-editor-appearance-control-code-more-opacity:0;--b3-theme-on-surface:rgb(91, 92, 93);width:640px">
    <div class="protyle-wysiwyg" data-appearance-fixture="native">${nativeCodeBlockDOM}</div>
    <div class="cm-editor" data-markdown-mode="visual"><div class="cm-content"><div class="cm-line cm-markra-code-line cm-markra-code-opening-line"><span class="protyle-action cm-markra-code-actions"><button class="protyle-action--first protyle-action__language markra-code-language-control cm-markra-code-header markra-code-language-label">javascript</button><span class="fn__flex-1"></span><button class="protyle-icon protyle-action__copy markra-code-copy-button" data-copied="false"><svg class="markra-code-copy-icon"></svg><svg class="markra-code-copy-check-icon"></svg></button><button class="protyle-icon protyle-action__menu markra-code-more-button"><svg></svg></button></span></div><div class="cm-line cm-markra-code-line cm-markra-code-content-line cm-markra-code-content-first"><span class="cm-markra-code-token hljs-keyword">const</span>&nbsp;markdownValue = 1;</div><div class="cm-line cm-markra-code-line cm-markra-code-content-line cm-markra-code-content-last">return markdownValue;</div><div class="cm-line cm-markra-code-line cm-markra-code-closing-line"></div></div></div>
</div>
${managementTreeHTML}
${managementDOM.window.document.querySelector("section").outerHTML}
`;
    const fixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "siyuan-markdown-layout-"));
    const fixturePath = path.join(fixtureDirectory, "layout.html");
    fs.writeFileSync(fixturePath, html);
    try {
        await window.loadFile(fixturePath);
    } finally {
        fs.rmSync(fixtureDirectory, {recursive: true});
    }
    const metrics = await window.webContents.executeJavaScript(`(() => {
        const editor = document.querySelector(".markdown-editor");
        const surface = document.querySelector(".markdown-editor__surface");
        const tableScroll = surface.querySelector(".markra-table-scroll");
        const table = tableScroll.querySelector("table");
        const widthModeWrapper = surface.querySelector("#table-width-mode-fixture");
        const widthModeScroll = widthModeWrapper.querySelector(".markra-table-scroll");
        const widthModeTable = widthModeWrapper.querySelector("table");
        const measureWidthMode = () => ({
            cellWidths: Array.from(widthModeTable.querySelectorAll("thead th"), (cell) =>
                cell.getBoundingClientRect().width),
            scrollWidth: widthModeScroll.getBoundingClientRect().width,
            tableDisplay: getComputedStyle(widthModeTable).display,
            tableLayout: getComputedStyle(widthModeTable).tableLayout,
            tableWidth: widthModeTable.getBoundingClientRect().width,
        });
        const autoWidthMode = measureWidthMode();
        widthModeWrapper.dataset.widthMode = "even";
        widthModeTable.dataset.widthMode = "even";
        const evenWidthMode = measureWidthMode();
        const imageFrame = surface.querySelector(".markra-image-frame");
        const selectedImageFrameStyle = getComputedStyle(imageFrame);
        const defaultImageFrame = surface.querySelector("#default-image-frame");
        const defaultImageRoot = defaultImageFrame.closest(".markra-image-node");
        const defaultImageHandle = defaultImageFrame.querySelector(".protyle-action__drag");
        const imageHandle = surface.querySelector(".markra-image-node .protyle-action__drag");
        const visualEditor = document.querySelector("#visual-editor");
        const largeImageEditor = document.querySelector("#large-image-editor");
        const enlargedImageFrame = largeImageEditor.querySelector(".markra-image-frame");
        const enlargedImage = largeImageEditor.querySelector("img");
        const visualHeading = visualEditor.querySelector(".cm-markra-h1 span");
        const visualHeadingLine = visualHeading.closest(".cm-markra-h1");
        const headingLevelLine = visualEditor.querySelector("#heading-level-line");
        const headingLevelButton = headingLevelLine.querySelector(".markra-heading-level-button");
        const headingLevelList = headingLevelLine.querySelector(".markra-heading-level-list");
        const headingLevelOptions = Array.from(headingLevelList.querySelectorAll(".markra-heading-level-option"));
        const visualLink = visualEditor.querySelector(".cm-markra-link");
        const visualList = visualEditor.querySelector(".cm-markra-list-item");
        const visualBlockquote = visualEditor.querySelector(".cm-markra-blockquote");
        const visualCalloutHeader = visualEditor.querySelector(".markra-callout-header");
        const visualCallout = visualEditor.querySelector(".cm-markra-callout");
        const texturedHeading = document.querySelector("#textured-heading-editor .cm-markra-h1");
        const texturedHeadingStyle = getComputedStyle(texturedHeading);
        const inlineThemeEditor = document.querySelector("#inline-theme-editor");
        const nativeInlineCode = inlineThemeEditor.querySelector('[data-type~="code"]');
        const nativeInlineHighlight = inlineThemeEditor.querySelector('[data-type~="mark"]');
        const markdownInlineCode = inlineThemeEditor.querySelector(".cm-markra-inline-code");
        const markdownInlineHighlight = inlineThemeEditor.querySelector(".cm-markra-highlight");
        const plainInlineText = inlineThemeEditor.querySelector(".plain-inline-text");
        const paragraphSpacingEditor = document.querySelector("#paragraph-spacing-editor");
        const nativeParagraphs = paragraphSpacingEditor.querySelectorAll(".protyle-wysiwyg > .p");
        const markdownParagraphs = paragraphSpacingEditor.querySelectorAll(".cm-markra-paragraph");
        const inactiveEmptyLine = paragraphSpacingEditor.querySelector(".cm-markra-empty-line:not(.cm-markra-active-empty-line)");
        const activeEmptyLine = paragraphSpacingEditor.querySelector("#active-empty-line");
        const emptyEditor = document.querySelector("#empty-editor");
        const emptyBody = emptyEditor.querySelector(".markdown-editor__body");
        const emptySurface = emptyEditor.querySelector(".markdown-editor__surface");
        const emptyBodyRect = emptyBody.getBoundingClientRect();
        const emptySurfaceRect = emptySurface.getBoundingClientRect();
        const documentScrollEditor = document.querySelector("#document-scroll-editor");
        const documentScroller = documentScrollEditor.querySelector(".markdown-editor__content");
        const codeMirrorScroller = documentScrollEditor.querySelector(".cm-scroller");
        const documentTitle = documentScrollEditor.querySelector(".markdown-editor__title");
        const verticalOwners = [documentScroller, codeMirrorScroller].filter((element) => {
            const overflowY = getComputedStyle(element).overflowY;
            return /(auto|scroll)/u.test(overflowY) && element.scrollHeight > element.clientHeight;
        });
        documentScroller.scrollTop = Math.min(240, documentScroller.scrollHeight - documentScroller.clientHeight);
        const documentScrollerRect = documentScroller.getBoundingClientRect();
        const mobileEditor = document.querySelector("#mobile-editor");
        const mobileBody = mobileEditor.querySelector(".markdown-editor__body");
        const mobileTableViewport = mobileEditor.querySelector(".markra-table-scroll");
        const headerEditor = document.querySelector("#header-editor");
        const headerCover = headerEditor.querySelector(".markdown-editor__cover");
        const headerTitle = headerEditor.querySelector(".protyle-title__input");
        const headerActionButtons = Array.from(headerEditor.querySelectorAll(".b3-button"));
        const toolbarEditor = document.querySelector("#toolbar-editor");
        const lightToolbarEditor = toolbarEditor.cloneNode(true);
        lightToolbarEditor.id = "light-toolbar-editor";
        lightToolbarEditor.style.setProperty("--b3-border-color", "rgb(210, 210, 210)");
        lightToolbarEditor.style.setProperty("--b3-list-hover", "rgb(235, 240, 250)");
        lightToolbarEditor.style.setProperty("--b3-theme-background", "rgb(255, 255, 255)");
        lightToolbarEditor.style.setProperty("--b3-theme-on-surface", "rgb(90, 90, 90)");
        lightToolbarEditor.style.setProperty("--b3-theme-primary", "rgb(30, 100, 220)");
        document.body.append(lightToolbarEditor);
        const toolbarSurface = toolbarEditor.querySelector(".markdown-editor__surface");
        const toolbar = toolbarEditor.querySelector(".markra-table-align-controls");
        const toolbarGroups = Array.from(toolbar.querySelectorAll(":scope > .markra-table-control-group"));
        const toolbarButtons = Array.from(toolbar.querySelectorAll(".markra-table-control"));
        const toolbarIcons = Array.from(toolbar.querySelectorAll(".markra-table-control-icon"));
        const selectedToolbarButton = toolbar.querySelector('[aria-pressed="true"]');
        const deleteToolbarButton = toolbar.querySelector(".markra-table-delete-table");
        const lightToolbar = lightToolbarEditor.querySelector(".markra-table-align-controls");
        const lightToolbarGroup = lightToolbar.querySelector(".markra-table-control-group");
        const lightSelectedToolbarButton = lightToolbar.querySelector('[aria-pressed="true"]');
        const lightDeleteToolbarButton = lightToolbar.querySelector(".markra-table-delete-table");
        const toolbarRect = toolbar.getBoundingClientRect();
        const toolbarTableRect = toolbarEditor.querySelector(".cm-markra-table").getBoundingClientRect();
        const codeParityEditor = document.querySelector("#code-parity-editor");
        const nativeCode = codeParityEditor.querySelector(".code-block");
        const nativeCodeContent = codeParityEditor.querySelector(".hljs");
        const nativeCodeAction = codeParityEditor.querySelector(".protyle-action__language");
        const nativeCodeActions = codeParityEditor.querySelector(".code-block > .protyle-action");
        const nativeCopyButton = codeParityEditor.querySelector(".protyle-wysiwyg .protyle-action__copy");
        const markdownCode = codeParityEditor.querySelector(".cm-markra-code-content-first");
        const markdownCodeLast = codeParityEditor.querySelector(".cm-markra-code-content-last");
        const markdownCodeContent = codeParityEditor.querySelector(".cm-markra-code-content-line");
        const markdownCodeActions = codeParityEditor.querySelector(".cm-markra-code-actions");
        const nativeCodeToken = codeParityEditor.querySelector(".protyle-wysiwyg .hljs-keyword");
        const markdownCodeToken = codeParityEditor.querySelector(".cm-markra-code-token.hljs-keyword");
        const markdownCodeAction = codeParityEditor.querySelector(".markra-code-language-label");
        const markdownCopyButton = codeParityEditor.querySelector(".markra-code-copy-button");
        const markdownCopyIcon = codeParityEditor.querySelector(".markra-code-copy-icon");
        const markdownCopyCheckIcon = codeParityEditor.querySelector(".markra-code-copy-check-icon");
        const imageFrameRect = imageFrame.getBoundingClientRect();
        const imageHandleRect = imageHandle.getBoundingClientRect();
        return {
            editorClientWidth: editor.clientWidth,
            editorScrollWidth: editor.scrollWidth,
            previewClientWidth: surface.clientWidth,
            previewScrollWidth: surface.scrollWidth,
            tableClientWidth: table.clientWidth,
            tableScrollWidth: table.scrollWidth,
            tableViewportClientWidth: tableScroll.clientWidth,
            tableViewportScrollWidth: tableScroll.scrollWidth,
            tableDisplay: getComputedStyle(table).display,
            tableHeight: table.getBoundingClientRect().height,
            autoWidthMode,
            evenWidthMode,
            imageFrameRight: imageFrameRect.right,
            imageHandleCenter: imageHandleRect.left + imageHandleRect.width / 2,
            selectedImageOutlineColor: selectedImageFrameStyle.outlineColor,
            selectedImageOutlineWidth: selectedImageFrameStyle.outlineWidth,
            defaultImageWidth: defaultImageFrame.getBoundingClientRect().width,
            defaultImageLeft: defaultImageFrame.getBoundingClientRect().left,
            defaultImageRight: defaultImageFrame.getBoundingClientRect().right,
            defaultImageRootWidth: defaultImageRoot.getBoundingClientRect().width,
            defaultImageHandleRight: defaultImageHandle.getBoundingClientRect().right,
            surfaceLeft: surface.getBoundingClientRect().left,
            surfaceRight: surface.getBoundingClientRect().right,
            visualEditorColor: getComputedStyle(visualEditor.querySelector(".cm-editor")).color,
            visualHeadingColor: getComputedStyle(visualHeading).color,
            visualHeadingBorderBottomWidth: getComputedStyle(visualHeadingLine).borderBottomWidth,
            visualHeadingBorderTopWidth: getComputedStyle(visualHeadingLine).borderTopWidth,
            visualHeadingFontSize: getComputedStyle(visualHeadingLine).fontSize,
            visualHeadingMarginBottom: getComputedStyle(visualHeadingLine).marginBottom,
            visualHeadingMarginTop: getComputedStyle(visualHeadingLine).marginTop,
            visualHeadingPaddingBottom: getComputedStyle(visualHeadingLine).paddingBottom,
            visualHeadingPaddingTop: getComputedStyle(visualHeadingLine).paddingTop,
            headingLevelButtonBorderRadius: getComputedStyle(headingLevelButton).borderRadius,
            headingLevelButtonHeight: headingLevelButton.getBoundingClientRect().height,
            headingLevelListBackground: getComputedStyle(headingLevelList).backgroundColor,
            headingLevelListPosition: getComputedStyle(headingLevelList).position,
            headingLevelListZIndex: getComputedStyle(headingLevelList).zIndex,
            headingLevelOptionTops: headingLevelOptions.map((option) => option.getBoundingClientRect().top),
            visualLinkColor: getComputedStyle(visualLink).color,
            visualListPaddingLeft: getComputedStyle(visualList).paddingLeft,
            visualBlockquotePaddingLeft: getComputedStyle(visualBlockquote).paddingLeft,
            visualCalloutHeaderColor: getComputedStyle(visualCalloutHeader).color,
            visualCalloutRadius: getComputedStyle(visualCallout).borderRadius,
            visualCalloutShadow: getComputedStyle(visualCallout).boxShadow,
            texturedHeadingBackgroundImage: texturedHeadingStyle.backgroundImage,
            texturedHeadingBackgroundClip: texturedHeadingStyle.backgroundClip,
            texturedHeadingWebkitBackgroundClip: texturedHeadingStyle.webkitBackgroundClip,
            texturedHeadingColor: texturedHeadingStyle.color,
            texturedHeadingTextFillColor: texturedHeadingStyle.webkitTextFillColor,
            texturedHeadingCaretColor: texturedHeadingStyle.caretColor,
            visualThemeHeadingColor: getComputedStyle(visualEditor).getPropertyValue("--b3-editor-appearance-block-heading-1-color").trim(),
            visualThemeLinkColor: getComputedStyle(visualEditor).getPropertyValue("--b3-editor-appearance-inline-link-color").trim(),
            nativeInlineCodeColor: getComputedStyle(nativeInlineCode).color,
            markdownInlineCodeColor: getComputedStyle(markdownInlineCode).color,
            nativeInlineHighlightColor: getComputedStyle(nativeInlineHighlight).color,
            markdownInlineHighlightColor: getComputedStyle(markdownInlineHighlight).color,
            plainInlineTextColor: getComputedStyle(plainInlineText).color,
            inlineThemeEditorColor: getComputedStyle(inlineThemeEditor.querySelector(".cm-editor")).color,
            nativeParagraphDistance: nativeParagraphs[1].getBoundingClientRect().top - nativeParagraphs[0].getBoundingClientRect().top,
            markdownParagraphDistance: markdownParagraphs[1].getBoundingClientRect().top - markdownParagraphs[0].getBoundingClientRect().top,
            inactiveEmptyLineHeight: inactiveEmptyLine.getBoundingClientRect().height,
            activeEmptyLineHeight: activeEmptyLine.getBoundingClientRect().height,
            markdownParagraphHeight: markdownParagraphs[0].getBoundingClientRect().height,
            enlargedImageFrameWidth: enlargedImageFrame.getBoundingClientRect().width,
            enlargedImageWidth: enlargedImage.getBoundingClientRect().width,
            enlargedImageHeight: enlargedImage.getBoundingClientRect().height,
            emptyBodyBottom: emptyBodyRect.bottom,
            emptyPreviewBottom: emptySurfaceRect.bottom,
            emptyPreviewHeight: emptySurfaceRect.height,
            documentScrollOwnerCount: verticalOwners.length,
            documentScrollOwnerIsContent: verticalOwners[0] === documentScroller,
            codeMirrorOverflowX: getComputedStyle(codeMirrorScroller).overflowX,
            codeMirrorOverflowY: getComputedStyle(codeMirrorScroller).overflowY,
            documentTitleLeavesViewport: documentTitle.getBoundingClientRect().bottom < documentScrollerRect.top,
            mobileBodyPaddingLeft: getComputedStyle(mobileBody).paddingLeft,
            mobileClientWidth: mobileEditor.clientWidth,
            mobileScrollWidth: mobileEditor.scrollWidth,
            mobileTableClientWidth: mobileTableViewport.clientWidth,
            mobileTableScrollWidth: mobileTableViewport.scrollWidth,
            headerCoverWidth: headerCover.getBoundingClientRect().width,
            headerEditorWidth: headerEditor.getBoundingClientRect().width,
            headerTitleColor: getComputedStyle(headerTitle).color,
            headerTitleFontFamily: getComputedStyle(headerTitle).fontFamily,
            headerTitleFontSize: getComputedStyle(headerTitle).fontSize,
            headerActionButtonTops: headerActionButtons.map((button) => button.getBoundingClientRect().top),
            lightToolbarBackground: getComputedStyle(lightToolbarGroup).backgroundColor,
            lightToolbarBorderColor: getComputedStyle(lightToolbarGroup).borderColor,
            lightToolbarDeleteColor: getComputedStyle(lightDeleteToolbarButton).color,
            lightToolbarSelectedColor: getComputedStyle(lightSelectedToolbarButton).color,
            lightToolbarThemeBackground: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-background").trim(),
            lightToolbarThemeBorder: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-border-color").trim(),
            lightToolbarThemeOnSurface: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-on-surface").trim(),
            lightToolbarThemePrimary: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-primary").trim(),
            toolbarButtonHeights: toolbarButtons.map((button) => button.getBoundingClientRect().height),
            toolbarButtonWidths: toolbarButtons.map((button) => button.getBoundingClientRect().width),
            toolbarDeleteColor: getComputedStyle(deleteToolbarButton).color,
            toolbarEditorClientWidth: toolbarEditor.clientWidth,
            toolbarEditorScrollWidth: toolbarEditor.scrollWidth,
            toolbarGap: getComputedStyle(toolbar).gap,
            toolbarGroupCount: toolbarGroups.length,
            toolbarGroupTops: toolbarGroups.map((group) => group.getBoundingClientRect().top),
            toolbarHeight: toolbarRect.height,
            toolbarTop: toolbarRect.top,
            toolbarIconHeights: toolbarIcons.map((icon) => icon.getBoundingClientRect().height),
            toolbarIconWidths: toolbarIcons.map((icon) => icon.getBoundingClientRect().width),
            toolbarRight: toolbarRect.right,
            toolbarScrollWidth: toolbar.scrollWidth,
            toolbarSelectedColor: getComputedStyle(selectedToolbarButton).color,
            toolbarSurfaceRight: toolbarSurface.getBoundingClientRect().right,
            toolbarTableTop: toolbarTableRect.top,
            toolbarTableBottom: toolbarTableRect.bottom,
            toolbarBottom: toolbarRect.bottom,
            toolbarThemeOnSurface: getComputedStyle(toolbarEditor).getPropertyValue("--b3-theme-on-surface").trim(),
            toolbarThemePrimary: getComputedStyle(toolbarEditor).getPropertyValue("--b3-theme-primary").trim(),
            nativeCodeBackground: getComputedStyle(nativeCode).backgroundColor,
            nativeCodeRadius: getComputedStyle(nativeCode).borderRadius,
            nativeCodeFontFamily: getComputedStyle(nativeCodeContent).fontFamily,
            nativeCodeActionColor: getComputedStyle(nativeCodeAction).color,
            markdownCodeBackground: getComputedStyle(markdownCode).backgroundColor,
            markdownCodeRadius: getComputedStyle(markdownCode).borderTopLeftRadius,
            markdownCodeFontFamily: getComputedStyle(markdownCodeContent).fontFamily,
            markdownCodeActionColor: getComputedStyle(markdownCodeAction).color,
            markdownCodeActionBorderRadius: getComputedStyle(markdownCodeAction).borderRadius,
            markdownCodeActionPaddingLeft: getComputedStyle(markdownCodeAction).paddingLeft,
            markdownCodeActionPaddingRight: getComputedStyle(markdownCodeAction).paddingRight,
            nativeCodeTextOffset: nativeCodeToken.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeTextOffset: markdownCodeToken.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeHeight: nativeCode.getBoundingClientRect().height,
            markdownCodeHeight: markdownCodeLast.getBoundingClientRect().bottom - markdownCode.getBoundingClientRect().top,
            markdownCodeMiddleRadius: getComputedStyle(markdownCodeLast).borderTopLeftRadius,
            markdownCodeBottomRadius: getComputedStyle(markdownCodeLast).borderBottomLeftRadius,
            nativeCodeLanguageLeft: nativeCodeAction.getBoundingClientRect().left - nativeCode.getBoundingClientRect().left,
            markdownCodeLanguageLeft: markdownCodeAction.getBoundingClientRect().left - markdownCode.getBoundingClientRect().left,
            nativeCodeLanguageTop: nativeCodeAction.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeLanguageTop: markdownCodeAction.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeActionRight: nativeCode.getBoundingClientRect().right - nativeCodeActions.getBoundingClientRect().right,
            markdownCodeActionRight: markdownCode.getBoundingClientRect().right - markdownCodeActions.getBoundingClientRect().right,
            nativeCodeActionTop: nativeCodeActions.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeActionTop: markdownCodeActions.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeTokenColor: getComputedStyle(nativeCodeToken).color,
            markdownCodeTokenColor: getComputedStyle(markdownCodeToken).color,
            markdownCopyIconDisplay: getComputedStyle(markdownCopyIcon).display,
            markdownCopyCheckIconDisplay: getComputedStyle(markdownCopyCheckIcon).display,
            markdownCopyButtonWidth: markdownCopyButton.getBoundingClientRect().width,
            markdownCopyButtonBorderRadius: getComputedStyle(markdownCopyButton).borderRadius,
            markdownCopyButtonColor: getComputedStyle(markdownCopyButton).color,
            markdownCopyButtonOpacity: getComputedStyle(markdownCopyButton).opacity,
            nativeCopyButtonWidth: nativeCopyButton.getBoundingClientRect().width,
        };
    })()`);
    const managementLayout = await window.webContents.executeJavaScript(`(() => {
        const tree = document.querySelector("[data-fixture='markdown-management-tree']");
        const historyPanel = document.querySelector("[data-fixture='markdown-history']");
        return {
            treeOrder: tree ? Array.from(tree.children).map((item) => item.getAttribute("data-kind")) : [],
            hasDeletedHistory: Boolean(historyPanel?.querySelector("[data-type='deletedMarkdownItem']")),
            historyWidth: historyPanel?.getBoundingClientRect().width || 0,
        };
    })()`);
    assert.deepEqual(managementLayout.treeOrder, ["native", "markdown"]);
    assert.equal(classifyMarkdownDrop(
        {notebook: "box", path: "/a.md"},
        {notebook: "box", directory: "/"},
    ), "sort");
    assert.equal(classifyMarkdownDrop(
        {notebook: "box", path: "/a.md"},
        {notebook: "box", directory: "/folder"},
    ), "move");
    assert.equal(managementLayout.hasDeletedHistory, true);
    assert.equal(managementLayout.historyWidth > 0, true);
    const closedTabs = [
        {children: {instance: "Editor", rootId: "20260820000000-native"}},
        {children: {instance: "MarkdownEditor", notebookId: "box", path: "/deleted.md"}},
    ];
    let restoredNative = false;
    const restored = await restoreRecentlyClosedTab({}, closedTabs, {
        validateMarkdown: async () => false,
        openMarkdown: async () => undefined,
        restoreNative: async () => { restoredNative = true; return true; },
        stale: () => undefined,
    });
    assert.equal(restored, true);
    assert.equal(restoredNative, true);
    assert.deepEqual(closedTabs, []);
    const keepsTabWidth = metrics.editorScrollWidth === metrics.editorClientWidth &&
        metrics.previewScrollWidth === metrics.previewClientWidth &&
        metrics.tableViewportClientWidth <= metrics.previewClientWidth &&
        metrics.tableViewportScrollWidth >= metrics.tableViewportClientWidth &&
        metrics.tableDisplay === "table" &&
        metrics.tableHeight > 0 &&
        Math.abs(metrics.imageFrameRight - metrics.imageHandleCenter) <= 3;
    if (!keepsTabWidth) {
        console.error("Markdown wide table escapes the tab width", metrics);
        app.exit(1);
        return;
    }
    const columnWidthModeWorks = metrics.autoWidthMode.tableDisplay === "table" &&
        metrics.autoWidthMode.tableLayout === "auto" &&
        Math.abs(metrics.autoWidthMode.cellWidths[0] - metrics.autoWidthMode.cellWidths[1]) > 1 &&
        metrics.evenWidthMode.tableDisplay === "table" &&
        metrics.evenWidthMode.tableLayout === "fixed" &&
        Math.abs(metrics.evenWidthMode.cellWidths[0] - metrics.evenWidthMode.cellWidths[1]) <= 1 &&
        Math.abs(metrics.evenWidthMode.tableWidth - metrics.evenWidthMode.scrollWidth) <= 1;
    if (!columnWidthModeWorks) {
        console.error("Markdown table column width mode does not affect layout", metrics);
        app.exit(1);
        return;
    }
    if (metrics.selectedImageOutlineWidth !== "1px" || metrics.selectedImageOutlineColor !== "rgb(66, 133, 244)") {
        console.error("Markdown selected image does not use the theme outline", metrics);
        app.exit(1);
        return;
    }
    if (metrics.defaultImageWidth < 400 || metrics.defaultImageWidth > metrics.previewClientWidth) {
        console.error("Markdown image does not use its intrinsic default size", metrics);
        app.exit(1);
        return;
    }
    const visualColorsMatch = metrics.visualEditorColor === "rgb(33, 33, 33)" &&
        metrics.visualHeadingColor === metrics.visualThemeHeadingColor &&
        metrics.visualHeadingFontSize === "36px" &&
        metrics.visualHeadingBorderBottomWidth === "3px" &&
        metrics.visualHeadingBorderTopWidth === "14px" &&
        metrics.visualHeadingMarginBottom === "0px" &&
        metrics.visualHeadingMarginTop === "0px" &&
        metrics.visualHeadingPaddingBottom === "7px" &&
        metrics.visualHeadingPaddingTop === "6px" &&
        metrics.visualLinkColor === metrics.visualThemeLinkColor;
    if (!visualColorsMatch) {
        console.error("Markdown visual mode leaks source syntax colors", metrics);
        app.exit(1);
        return;
    }
    const texturedHeadingPaints = metrics.texturedHeadingBackgroundImage.includes("linear-gradient") &&
        metrics.texturedHeadingBackgroundClip === "text" &&
        metrics.texturedHeadingWebkitBackgroundClip === "text" &&
        metrics.texturedHeadingColor === "rgba(0, 0, 0, 0)" &&
        metrics.texturedHeadingTextFillColor === "rgba(0, 0, 0, 0)" &&
        metrics.texturedHeadingCaretColor === "rgba(0, 0, 0, 0)";
    if (!texturedHeadingPaints) {
        console.error("Markdown heading does not preserve textured theme paint", metrics);
        app.exit(1);
        return;
    }
    const headingLevelMenuFloats = metrics.headingLevelListPosition === "absolute" &&
        metrics.headingLevelListZIndex !== "auto" &&
        metrics.headingLevelListBackground === "rgb(255, 255, 255)" &&
        metrics.headingLevelButtonBorderRadius !== "0px" &&
        metrics.headingLevelButtonHeight >= 24 &&
        new Set(metrics.headingLevelOptionTops).size === metrics.headingLevelOptionTops.length;
    if (!headingLevelMenuFloats) {
        console.error("Markdown heading-level control renders as unstyled inline buttons", metrics);
        app.exit(1);
        return;
    }
    if (metrics.visualListPaddingLeft !== "0px" || metrics.visualBlockquotePaddingLeft !== "10px" ||
        metrics.visualCalloutHeaderColor !== "rgb(18, 88, 148)" || metrics.visualCalloutRadius !== "11px" ||
        metrics.visualCalloutShadow !== "rgb(22, 92, 152) 0px 0px 0px 2px inset") {
        console.error("Markdown list or blockquote does not use native content padding", metrics);
        app.exit(1);
        return;
    }
    const inlineThemeColorsMatch = metrics.markdownInlineCodeColor === metrics.nativeInlineCodeColor &&
        metrics.markdownInlineHighlightColor === metrics.nativeInlineHighlightColor &&
        metrics.plainInlineTextColor === metrics.inlineThemeEditorColor;
    if (!inlineThemeColorsMatch) {
        console.error("Markdown semantic inline colors differ from the native third-party theme", metrics);
        app.exit(1);
        return;
    }
    if (Math.abs(metrics.markdownParagraphDistance - metrics.nativeParagraphDistance) > 1) {
        console.error("Markdown authored blank lines add extra paragraph spacing", metrics);
        app.exit(1);
        return;
    }
    if (metrics.inactiveEmptyLineHeight > 1 ||
        Math.abs(metrics.activeEmptyLineHeight - metrics.markdownParagraphHeight) > 1) {
        console.error("Active Markdown empty line does not preserve a full-height caret row", metrics);
        app.exit(1);
        return;
    }
    if (metrics.documentScrollOwnerCount !== 1 || !metrics.documentScrollOwnerIsContent ||
        metrics.codeMirrorOverflowX !== "visible" || metrics.codeMirrorOverflowY !== "visible" ||
        !metrics.documentTitleLeavesViewport) {
        console.error("Markdown document does not use one native-like vertical scroll owner", metrics);
        app.exit(1);
        return;
    }
    const enlargedRatio = metrics.enlargedImageWidth / metrics.enlargedImageHeight;
    if (Math.abs(metrics.enlargedImageWidth - metrics.enlargedImageFrameWidth) > 1 ||
        Math.abs(enlargedRatio - 506 / 538) > 0.01) {
        console.error("Markdown enlarged image is clipped or does not fill its frame", metrics);
        app.exit(1);
        return;
    }
    const fillsEditableBody = metrics.emptyPreviewBottom >= metrics.emptyBodyBottom - 1 &&
        metrics.emptyPreviewHeight > 300;
    if (!fillsEditableBody) {
        console.error("Markdown empty preview does not fill the editable body", metrics);
        app.exit(1);
        return;
    }
    const keepsMobileWidth = metrics.mobileClientWidth === 375 &&
        metrics.mobileScrollWidth === metrics.mobileClientWidth &&
        metrics.mobileTableClientWidth < metrics.mobileTableScrollWidth &&
        metrics.mobileBodyPaddingLeft === "16px";
    if (!keepsMobileWidth) {
        console.error("Markdown mobile layout escapes the viewport", metrics);
        app.exit(1);
        return;
    }
    const headerUsesNativeTheme = Math.abs(metrics.headerCoverWidth - metrics.headerEditorWidth) <= 1 &&
        metrics.headerTitleColor === "rgb(235, 235, 235)" &&
        metrics.headerTitleFontFamily === "serif" &&
        metrics.headerTitleFontSize === "36px";
    if (!headerUsesNativeTheme) {
        console.error("Markdown metadata header does not follow the native theme and layout", metrics);
        app.exit(1);
        return;
    }
    const headerActionsStayTogether = metrics.headerActionButtonTops.length === 3 &&
        metrics.headerActionButtonTops.every((top) => Math.abs(top - metrics.headerActionButtonTops[0]) <= 0.5);
    if (!headerActionsStayTogether) {
        console.error("Markdown metadata actions split across rows after adding a tag", metrics);
        app.exit(1);
        return;
    }
    const toolbarFitsNarrowEditor = metrics.toolbarEditorScrollWidth === metrics.toolbarEditorClientWidth &&
        metrics.toolbarScrollWidth <= metrics.toolbarEditorClientWidth &&
        metrics.toolbarRight <= metrics.toolbarSurfaceRight + 1 &&
        metrics.toolbarTableTop <= metrics.toolbarTop + 1 &&
        metrics.toolbarTableBottom >= metrics.toolbarBottom - 1 &&
        metrics.toolbarGroupCount === 4 &&
        metrics.toolbarGroupTops.every((top) => Math.abs(top - metrics.toolbarGroupTops[0]) <= 0.5) &&
        metrics.toolbarGap === "8px" &&
        metrics.toolbarButtonWidths.every((width) => Math.abs(width - 24) <= 0.5) &&
        metrics.toolbarButtonHeights.every((height) => Math.abs(height - 24) <= 0.5) &&
        metrics.toolbarIconWidths.every((width) => Math.abs(width - 16) <= 0.5) &&
        metrics.toolbarIconHeights.every((height) => Math.abs(height - 16) <= 0.5) &&
        metrics.toolbarSelectedColor === metrics.toolbarThemePrimary &&
        metrics.toolbarDeleteColor === metrics.toolbarThemeOnSurface &&
        metrics.lightToolbarBackground === metrics.lightToolbarThemeBackground &&
        metrics.lightToolbarBorderColor === metrics.lightToolbarThemeBorder &&
        metrics.lightToolbarSelectedColor === metrics.lightToolbarThemePrimary &&
        metrics.lightToolbarDeleteColor === metrics.lightToolbarThemeOnSurface;
    if (!toolbarFitsNarrowEditor) {
        console.error("Markdown table toolbar does not fit the narrow editor", metrics);
        app.exit(1);
        return;
    }
    const codeBlockUsesNativeTheme = metrics.markdownCodeBackground === metrics.nativeCodeBackground &&
        metrics.markdownCodeRadius === metrics.nativeCodeRadius &&
        metrics.markdownCodeFontFamily === metrics.nativeCodeFontFamily &&
        metrics.markdownCodeActionColor === metrics.nativeCodeActionColor &&
        metrics.markdownCodeActionBorderRadius === "3px" &&
        metrics.markdownCodeActionPaddingLeft === "8px" &&
        metrics.markdownCodeActionPaddingRight === "8px" &&
        Math.abs(metrics.markdownCodeTextOffset - metrics.nativeCodeTextOffset) <= 1 &&
        Math.abs(metrics.markdownCodeHeight - metrics.nativeCodeHeight) <= 1 &&
        metrics.markdownCodeMiddleRadius === "0px" &&
        metrics.markdownCodeBottomRadius === metrics.nativeCodeRadius &&
        Math.abs(metrics.markdownCodeLanguageLeft - metrics.nativeCodeLanguageLeft) <= 1 &&
        Math.abs(metrics.markdownCodeLanguageTop - metrics.nativeCodeLanguageTop) <= 1 &&
        Math.abs(metrics.markdownCodeActionRight - metrics.nativeCodeActionRight) <= 1 &&
        Math.abs(metrics.markdownCodeActionTop - metrics.nativeCodeActionTop) <= 1 &&
        metrics.markdownCodeTokenColor === metrics.nativeCodeTokenColor &&
        metrics.markdownCopyIconDisplay !== "none" &&
        metrics.markdownCopyCheckIconDisplay === "none" &&
        metrics.markdownCopyButtonBorderRadius === "8px 0px 0px 8px" &&
        metrics.markdownCopyButtonColor === "rgb(41, 42, 43)" &&
        metrics.markdownCopyButtonOpacity === "0" &&
        Math.abs(metrics.markdownCopyButtonWidth - metrics.nativeCopyButtonWidth) <= 1;
    if (!codeBlockUsesNativeTheme) {
        console.error("Markdown code block does not use the native theme", metrics);
        app.exit(1);
        return;
    }
    console.log("Markdown wide table stays within the tab width", metrics);
    app.exit(0);
}).catch((error) => {
    console.error(error);
    app.exit(1);
});
