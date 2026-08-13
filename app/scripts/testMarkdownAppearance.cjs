const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const connect = async () => {
    const targets = await fetch("http://127.0.0.1:9222/json/list").then((response) => response.json());
    const target = targets.find((item) => item.type === "page" && /QingYu|轻语/u.test(item.title));
    assert.ok(target?.webSocketDebuggerUrl, "A running QingYu page with remote debugging is required");
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
        socket.addEventListener("open", resolve, {once: true});
        socket.addEventListener("error", reject, {once: true});
    });
    let messageId = 0;
    const pending = new Map();
    const rejectPending = (error) => {
        for (const {reject, timer} of pending.values()) {
            clearTimeout(timer);
            reject(error);
        }
        pending.clear();
    };
    socket.addEventListener("close", () => rejectPending(new Error("The QingYu debugging connection closed")));
    socket.addEventListener("error", () => rejectPending(new Error("The QingYu debugging connection failed")));
    socket.addEventListener("message", (event) => {
        const message = JSON.parse(String(event.data));
        const request = pending.get(message.id);
        if (!request) return;
        pending.delete(message.id);
        clearTimeout(request.timer);
        if (message.error) request.reject(new Error(message.error.message));
        else request.resolve(message.result);
    });
    const call = (method, params = {}) => new Promise((resolve, reject) => {
        const id = ++messageId;
        const timer = setTimeout(() => {
            pending.delete(id);
            reject(new Error(`The QingYu debugging command timed out: ${method}`));
        }, 30000);
        pending.set(id, {reject, resolve, timer});
        socket.send(JSON.stringify({id, method, params}));
    });
    return {call, socket};
};

const evaluate = async (call, expression) => {
    let result;
    try {
        result = await call("Runtime.evaluate", {expression, returnByValue: true, awaitPromise: true});
    } catch (error) {
        throw new Error(`${error.message}; expression: ${expression.slice(0, 160)}`);
    }
    if (result.exceptionDetails) {
        throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    }
    return result.result.value;
};

const matrix = [
    {theme: "daylight", mode: "visual", platform: "desktop", width: 500, input: "mouse"},
    {theme: "midnight", mode: "visual", platform: "desktop", width: 500, input: "keyboard"},
    {theme: "standard-third-party", mode: "visual", platform: "desktop", width: 320, input: "mouse"},
    {theme: "daylight", mode: "visual", platform: "mobile", width: 375, input: "touch"},
    {theme: "midnight", mode: "source", platform: "desktop", width: 500, input: "keyboard"},
    {theme: "standard-third-party", mode: "source", platform: "mobile", width: 375, input: "touch"},
];

const main = async () => {
    const {call, socket} = await connect();
    const outputDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-markdown-appearance-"));
    const reports = [];
    let emptyReport = null;
    try {
        await call("Page.enable");
        await call("Runtime.enable");
        const available = await evaluate(call, "Boolean(window.__siyuanMarkdownAppearanceTest)");
        assert.equal(available, true, "The development Markdown appearance runtime harness is not installed");
        for (const [index, row] of matrix.entries()) {
            console.log("Markdown appearance row", index, row);
            const viewportWidth = Math.max(760, row.width * 2 + 64);
            await call("Emulation.setDeviceMetricsOverride", {
                deviceScaleFactor: 1,
                height: 2400,
                mobile: false,
                width: viewportWidth,
            });
            await call("Emulation.setTouchEmulationEnabled", {enabled: row.input === "touch"});
            await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(row)}).then(() => true)`);
            await evaluate(call, `window.__siyuanMarkdownAppearanceTest.setTheme(${JSON.stringify(row.theme)})`);
            const stateReports = [{state: "base", report: await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measure()")}];
            const behavior = await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measureDocumentBehavior()");
            assert.equal(behavior.documentScrollOwnerCount, 1, "Markdown must expose one document-level vertical scroller");
            assert.equal(behavior.documentScrollOwnerIsContent, true, "Markdown content must own document scrolling");
            assert.equal(behavior.titleLeavesViewport, true, "Markdown title must leave the viewport with the document");
            assert.ok(behavior.renderedLineCount > 0, "CodeMirror must render visible document lines");
            const continuity = await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measureModeContinuity()");
            assert.equal(continuity.sameView, true, "Markdown mode changes must preserve the EditorView");
            assert.equal(continuity.anchorPositionAfter, continuity.anchorPositionBefore, "Markdown mode changes must preserve the document anchor");
            assert.ok(continuity.anchorOffsetDifference <= 1, "Markdown mode changes must preserve the anchor viewport offset");
            if (row.theme === "midnight" && row.mode === "visual") {
                for (const contractId of ["block.paragraph", "block.heading-1"]) {
                    const measurement = stateReports[0].report.measurements.find((item) => item.contractId === contractId);
                    assert.ok(measurement?.native?.styles.color, `Missing native dark-theme color for ${contractId}`);
                    assert.equal(
                        measurement.markdown?.styles.color,
                        measurement.native.styles.color,
                        `Markdown dark-theme color differs for ${contractId}`,
                    );
                }
            }
            const overflow = await evaluate(call, `(() => {
                const shell = document.querySelector(".markdown-appearance-runtime__markdown");
                return {clientWidth: shell?.clientWidth ?? 0, scrollWidth: shell?.scrollWidth ?? 0};
            })()`);
            assert.ok(
                overflow.scrollWidth <= overflow.clientWidth + 1,
                `Markdown fixture overflows horizontally: ${JSON.stringify({...row, ...overflow})}`,
            );
            const screenshot = await call("Page.captureScreenshot", {format: "png", fromSurface: true});
            fs.writeFileSync(
                path.join(outputDirectory, `${index}-${row.theme}-${row.mode}-${row.platform}-visual.png`),
                screenshot.data,
                "base64",
            );
            if (row.platform === "mobile" && row.mode === "visual") {
                const tableToolbarVisibility = await evaluate(call, `(async () => {
                    const toolbar = document.querySelector(".markdown-appearance-runtime__markdown .markra-table-align-controls");
                    const table = document.querySelector(".markdown-appearance-runtime__markdown .cm-markra-table");
                    const baseOpacity = toolbar ? Number(getComputedStyle(toolbar).opacity) : -1;
                    table?.focus();
                    await new Promise((resolve) => setTimeout(resolve, 200));
                    const focusedOpacity = toolbar ? Number(getComputedStyle(toolbar).opacity) : -1;
                    return {baseOpacity, focusedOpacity};
                })()`);
                assert.equal(tableToolbarVisibility.baseOpacity, 0, "Mobile table toolbar must be hidden before interaction");
                assert.equal(tableToolbarVisibility.focusedOpacity, 1, "Mobile table toolbar must appear when the table is focused");
            }
            for (const state of ["focus", "selected", "drag", "clipboard", "error", "media", "expanded"]) {
                console.log("Markdown appearance state", index, state);
                await evaluate(call, `window.__siyuanMarkdownAppearanceTest.interact(${JSON.stringify(state)})`);
                stateReports.push({state, report: await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measure()")});
            }
            reports.push({...row, behavior, continuity, overflow, stateReports});
            fs.writeFileSync(
                path.join(outputDirectory, `${index}-${row.theme}-${row.mode}-${row.platform}.json`),
                JSON.stringify({row, behavior, continuity, overflow, stateReports}, null, 2),
            );
            await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
        }
        const emptyOptions = {...matrix[0], markdown: ""};
        await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(emptyOptions)}).then(() => true)`);
        await evaluate(call, `window.__siyuanMarkdownAppearanceTest.setTheme(${JSON.stringify(matrix[0].theme)})`);
        emptyReport = await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measure()");
        const emptyScreenshot = await call("Page.captureScreenshot", {format: "png", fromSurface: true});
        fs.writeFileSync(path.join(outputDirectory, "empty-daylight-visual-desktop.png"), emptyScreenshot.data, "base64");
        fs.writeFileSync(path.join(outputDirectory, "empty-daylight-visual-desktop.json"), JSON.stringify(emptyReport, null, 2));
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
    } finally {
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest?.destroy() ").catch(() => undefined);
        await call("Emulation.setTouchEmulationEnabled", {enabled: false}).catch(() => undefined);
        await call("Emulation.clearDeviceMetricsOverride").catch(() => undefined);
        socket.close();
    }

    const allReports = [
        ...reports.flatMap(({stateReports}) => stateReports.map(({report}) => report)),
        ...(emptyReport ? [emptyReport] : []),
    ];
    const contractCount = allReports[0]?.contractCount ?? 0;
    const seen = new Set(allReports.flatMap((report) => report.measurements
        .filter((measurement) => measurement.markdown)
        .map((measurement) => measurement.contractId)));
    const requiredRuntimeContracts = [
        "shell.document",
        "editor.visual",
        "editor.source",
        "block.code",
        "block.table",
        "media.image",
        "control.code-language",
        "control.table-toolbar",
    ];
    const missingRequired = requiredRuntimeContracts.filter((id) => !seen.has(id));
    assert.equal(matrix.length, 6);
    assert.ok(contractCount >= 50, `Unexpected appearance contract count: ${contractCount}`);
    assert.deepEqual(missingRequired, [], `Required runtime contracts were not rendered: ${missingRequired.join(", ")}`);
    const knownContracts = new Set(allReports.flatMap((report) => report.measurements.map((measurement) => measurement.contractId)));
    const parityReports = reports.flatMap(({stateReports}) => stateReports
        .filter(({state}) => ["base", "focus", "selected"].includes(state))
        .map(({report}) => report));
    const summary = {
        contractCount,
        matrixRows: reports.length,
        maximumGeometryDifference: Math.max(...parityReports.map((report) => report.maximumGeometryDifference)),
        outputDirectory,
        screenshotCount: fs.readdirSync(outputDirectory).filter((name) => name.endsWith(".png")).length,
        seenContracts: seen.size,
        uncovered: [...knownContracts].filter((id) => !seen.has(id)),
    };
    fs.writeFileSync(path.join(outputDirectory, "summary.json"), JSON.stringify(summary, null, 2));
    assert.equal(summary.screenshotCount, matrix.length + 1);
    assert.equal(summary.seenContracts, summary.contractCount, `Appearance contracts not rendered: ${summary.uncovered.join(", ")}`);
    assert.ok(
        summary.maximumGeometryDifference <= 12,
        `Native-equivalent geometry differs by ${summary.maximumGeometryDifference}px`,
    );
    console.log("Markdown appearance runtime matrix passed", summary);
};

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
