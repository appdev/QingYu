const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const assert = require("node:assert/strict");
const {app, BrowserWindow} = require("electron");
const sass = require("sass");
const esbuild = require("node:module").createRequire(require.resolve("tsx/cjs"))("esbuild");

const appRoot = path.resolve(__dirname, "..");
const reportDirectory = process.env.QINGYU_ANNOTATION_REPORT_DIR || path.join(os.tmpdir(), "qingyu-annotation-verification");
fs.mkdirSync(reportDirectory, {recursive: true});

const renderer = async ({css, theme}) => {
    const {EditorView, minimalSetup, EditorState, Compartment, annotationExtension, annotationField,
        annotationSelectionToolbar, MarkdownAnnotationSidebar, anchorAt, liveMarkdown, markraLanguage,
        formattingPlugin, tablePreviewPlugin, mathPreviewPlugin, codeBlockPreviewPlugin, markdownHostAdapter,
        MarkdownDocumentScrollController} = window.annotationRuntime;
    document.head.innerHTML = "";
    const themeStyle = document.createElement("style");
    themeStyle.textContent = theme;
    document.head.append(themeStyle);
    const style = document.createElement("style");
    style.textContent = css + "html,body{margin:0;height:100%;} .markdown-editor{height:100vh;display:flex;flex-direction:column;} .markdown-editor__content{flex:1;min-height:0;overflow:auto;} .cm-scroller{overflow:visible!important;} .cm-editor{height:auto!important;} .markdown-editor__body{padding:24px;}";
    document.head.append(style);
    document.body.innerHTML = '<div class="markdown-editor"><div style="height:40px;flex-shrink:0"></div><div class="markdown-editor__content"><div class="markdown-editor__body"><div class="markdown-editor__surface"></div></div></div></div>';
    const root = document.querySelector(".markdown-editor");
    const scroll = document.querySelector(".markdown-editor__content");
    const surface = document.querySelector(".markdown-editor__surface");
    const labels = {title: "批注", add: "添加批注", orphan: "原文已失效", reattach: "重新关联", select: "请在源码中选中文字", all: "全部批注", margin: "旁注", edit: "编辑", remove: "删除", save: "保存", cancel: "取消", close: "关闭"};
    const text = Array.from({length: 200}, (_, index) => `${String(index).padStart(3, "0")} ${"读书使人思考，笔记帮助理解。".repeat(4)}思考\n\n`).join("");
    const records = [];
    let offset = 0;
    for (let index = 0; index < 200; index++) {
        records.push({...anchorAt(text, offset + 4, offset + 12), id: `a${index}`, note: `第 ${index + 1} 条读书笔记。`, createdAt: 1, updatedAt: 1});
        offset = text.indexOf("\n\n", offset) + 2;
    }
    const mode = new Compartment();
    let sidebar;
    const adapter = {
        createIcon: (_name, className, owner) => { const icon = owner.createElementNS("http://www.w3.org/2000/svg", "svg"); icon.setAttribute("class", className); return icon; },
        notifyError: (message) => { throw new Error(message); }, openLink() {}, positionPopover() {},
        renderMath: (source) => { const element = document.createElement("span"); element.textContent = source; return element; },
        renderMermaid: async (source) => { const element = document.createElement("span"); element.textContent = source; return element; },
        resolveImageSource: (source) => source, saveClipboardAssets: async () => [],
    };
    const view = new EditorView({doc: text, parent: surface, extensions: [minimalSetup, markdownHostAdapter(adapter),
        mode.of(markraLanguage), EditorView.lineWrapping, annotationExtension(records),
        annotationSelectionToolbar(labels.add, () => sidebar.add()), EditorView.updateListener.of((update) => sidebar?.update(update))]});
    const documentScroll = new MarkdownDocumentScrollController(() => view, scroll);
    sidebar = new MarkdownAnnotationSidebar(view, root, scroll, labels, () => view.dispatch({effects: mode.reconfigure(markraLanguage)}));
    const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await frame();
    await frame();
    const summarize = (values) => {
        const sorted = [...values].sort((a, b) => a - b);
        return {medianMs: sorted[Math.floor(sorted.length / 2)], p95Ms: sorted[Math.ceil(sorted.length * 0.95) - 1]};
    };
    const transactions = (enabled) => {
        let state = EditorState.create({doc: text, extensions: enabled ? annotationExtension(records) : []});
        const samples = [];
        for (let index = 0; index < 35; index++) {
            const start = performance.now();
            state = state.update({changes: {from: 8, insert: "字"}}).state;
            if (index >= 5) samples.push(performance.now() - start);
        }
        return summarize(samples);
    };
    const baseline = transactions(false);
    const annotated = transactions(true);
    const navigation = [];
    const longTasks = [];
    const observer = new PerformanceObserver((list) => longTasks.push(...list.getEntries().map((entry) => entry.duration)));
    observer.observe({type: "longtask", buffered: false});
    for (let index = 0; index < 30; index++) {
        const start = performance.now();
        sidebar.select(`a${(index * 7) % 200}`);
        await frame();
        navigation.push(performance.now() - start);
        const target = view.state.field(annotationField).find((record) => record.id === `a${(index * 7) % 200}`);
        const coords = view.coordsAtPos(target.from);
        const viewport = scroll.getBoundingClientRect();
        if (!coords || coords.bottom < viewport.top || coords.top > viewport.bottom) throw new Error("navigation did not reveal the source text");
    }
    const renderedCards = document.querySelectorAll(".markdown-annotation-card").length;
    const activeCard = document.querySelector(".markdown-annotation-card--active");
    const activeBounds = activeCard.getBoundingClientRect();
    const panelBounds = document.querySelector(".markdown-annotations__cards").getBoundingClientRect();
    if (activeBounds.bottom <= panelBounds.top || activeBounds.top >= panelBounds.bottom) throw new Error("active annotation is outside the visible panel");
    const modes = [];
    for (const visual of [false, true]) {
        view.dispatch({effects: mode.reconfigure(visual ? liveMarkdown({plugins: [formattingPlugin(), tablePreviewPlugin(), mathPreviewPlugin(), codeBlockPreviewPlugin()]}) : markraLanguage)});
        await frame();
        modes.push({mode: visual ? "visual" : "source", records: view.state.field(annotationField).length});
    }
    sidebar.select("a0");
    await frame();
    view.dispatch({selection: {anchor: 4, head: 12}});
    sidebar.add();
    await frame();
    const input = document.querySelector(".markdown-annotation-card textarea");
    if (!input || document.activeElement !== input) throw new Error("annotation draft did not receive focus");
    input.value = "新增笔记，检查输入与焦点。";
    input.dispatchEvent(new Event("input", {bubbles: true}));
    if (view.state.field(annotationField).at(-1).note !== input.value) throw new Error("note edit was not stored");
    input.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    await frame();
    if (view.state.field(annotationField).length !== 200) throw new Error("cancelled draft was not removed");
    const card = document.querySelector(".markdown-annotation-card");
    const appearance = {background: getComputedStyle(card).backgroundColor, color: getComputedStyle(card).color,
        mark: getComputedStyle(document.querySelector(".markdown-annotation-mark")).backgroundColor};
    const savedText = view.state.doc.toString();
    const savedRecords = view.state.field(annotationField);
    themeStyle.textContent += ":root { --b3-theme-surface: rgb(220, 230, 240); --b3-theme-on-background: rgb(25, 45, 65); }";
    await frame();
    if (getComputedStyle(card).backgroundColor !== "rgb(220, 230, 240)" || view.state.doc.toString() !== savedText ||
        view.state.field(annotationField) !== savedRecords) throw new Error("theme switch altered annotations or failed to recolor cards");
    themeStyle.textContent = theme;
    root.style.width = "375px";
    sidebar.schedule();
    await frame();
    await frame();
    const narrowPanel = document.querySelector(".markdown-annotations").getBoundingClientRect();
    if (!root.classList.contains("markdown-editor--annotations-narrow") || narrowPanel.width > 343) throw new Error("narrow annotation panel overflow");
    const narrow = {width: narrowPanel.width, cards: document.querySelectorAll(".markdown-annotation-card").length};
    root.style.width = "";
    await frame();
    await frame();
    observer.disconnect();
    window.annotationFixture = {view, sidebar, mode, markraLanguage, frame, root, scroll,
        destroy: () => { sidebar.destroy(); documentScroll.destroy(); view.destroy(); }};
    return {characters: text.length, chineseCharacters: (text.match(/\p{Script=Han}/gu) || []).length, annotations: records.length,
        samples: 30, baseline, annotated, incrementalP95Ms: annotated.p95Ms - baseline.p95Ms,
        navigation: summarize(navigation), longTasks, renderedCards, modes, appearance, narrow, themeSwitchPreservedState: true};
};

(async () => {
    await app.whenReady();
    const window = new BrowserWindow({show: false, width: 1280, height: 900,
        webPreferences: {nodeIntegration: true, contextIsolation: false, backgroundThrottling: false}});
    const css = ["business/_markdown.scss", "component/_button.scss", "component/_text-field.scss", "util/_reset.scss", "util/_scroll.scss"]
        .map((file) => sass.compile(path.join(appRoot, "src/assets/scss", file), {logger: sass.Logger.silent}).css).join("\n");
    const report = {machine: {platform: process.platform, arch: process.arch, cpu: os.cpus()[0].model,
        electron: process.versions.electron, chrome: process.versions.chrome}, viewport: {width: 1280, height: 900}, results: []};
    await window.loadURL("about:blank");
    const bundle = await esbuild.build({stdin: {resolveDir: appRoot, contents: `
        export {EditorView, minimalSetup} from "codemirror";
        export {EditorState, Compartment} from "@codemirror/state";
        export {annotationExtension, annotationField, annotationSelectionToolbar} from "./src/markdown/annotations/extension";
        export {MarkdownAnnotationSidebar} from "./src/markdown/annotations/sidebar";
        export {anchorAt} from "./src/markdown/annotations/anchors";
        export {liveMarkdown, markraLanguage, formattingPlugin, tablePreviewPlugin, mathPreviewPlugin, codeBlockPreviewPlugin} from "./src/markdown/markra-core/codemirror";
        export {markdownHostAdapter} from "./src/markdown/markra-core/adapter";
        export {MarkdownDocumentScrollController} from "./src/markdown/documentScroll";
    `}, bundle: true, write: false, platform: "browser", format: "iife", globalName: "annotationRuntime"});
    await window.webContents.executeJavaScript(bundle.outputFiles[0].text + ";window.annotationRuntime = annotationRuntime; void 0;");
    for (const themeName of ["daylight", "midnight", "custom-light", "custom-dark"]) {
        const base = themeName === "custom-light" ? "daylight" : themeName === "custom-dark" ? "midnight" : themeName;
        let theme = fs.readFileSync(path.join(appRoot, `appearance/themes/${base}/theme.css`), "utf8");
        if (themeName === "custom-light") theme += ":root { --b3-theme-surface: rgb(236, 243, 234); --b3-theme-on-background: rgb(31, 54, 35); --b3-theme-primary: rgb(50, 105, 68); }";
        if (themeName === "custom-dark") theme += ":root { --b3-theme-surface: rgb(46, 42, 58); --b3-theme-on-background: rgb(228, 219, 242); --b3-theme-primary: rgb(173, 151, 222); }";
        const result = await window.webContents.executeJavaScript(`(${renderer.toString()})(${JSON.stringify({appRoot, css, theme})})`);
        report.results.push({theme: themeName, ...result});
        fs.writeFileSync(path.join(reportDirectory, `${themeName}.png`), (await window.webContents.capturePage()).toPNG());
        await window.webContents.executeJavaScript("window.annotationFixture.destroy()");
    }
    fs.writeFileSync(path.join(reportDirectory, "report.json"), JSON.stringify(report, null, 2));
    console.log(JSON.stringify({reportDirectory, ...report}, null, 2));
    for (const result of report.results) {
        assert.equal(result.annotations, 200);
        assert.ok(result.incrementalP95Ms <= 16, "annotation transaction budget exceeded");
        assert.ok(result.navigation.p95Ms <= 100, "annotation navigation budget exceeded");
        assert.ok(result.renderedCards < 80, "margin rendered too many cards");
        assert.ok(result.longTasks.every((duration) => duration <= 50), "annotation long task budget exceeded");
    }
    window.destroy();
    app.quit();
})().catch((error) => { console.error(error); app.exit(1); });
