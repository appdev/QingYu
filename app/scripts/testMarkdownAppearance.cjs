const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const debugPort = process.env.QINGYU_MARKDOWN_DEBUG_PORT || "9222";

const connect = async () => {
    const targets = await fetch(`http://127.0.0.1:${debugPort}/json/list`).then((response) => response.json());
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
    {theme: "savor", mode: "visual", platform: "desktop", width: 500, input: "mouse"},
    {theme: "daylight", mode: "visual", platform: "mobile", width: 375, input: "touch"},
    {theme: "midnight", mode: "source", platform: "desktop", width: 500, input: "keyboard"},
    {theme: "standard-third-party", mode: "source", platform: "mobile", width: 375, input: "touch"},
];

const showcaseDirectory = path.resolve(__dirname, "../testdata/markdown");
const showcaseSource = fs.readFileSync(path.join(showcaseDirectory, "format-showcase.md"), "utf8");
const showcaseSvg = fs.readFileSync(path.join(showcaseDirectory, "format-showcase.svg"), "utf8");
const showcaseImageDataUrl = `data:image/svg+xml,${encodeURIComponent(showcaseSvg)}`;
const showcaseMarkdown = showcaseSource.replaceAll("./format-showcase.svg", showcaseImageDataUrl);
const showcaseScreenshotIds = ["Q04", "Q05", "R01", "T01", "T03", "T04", "T05", "T06", "K02", "M02", "M03", "P01", "X03"];

const showcaseCase = (caseId) => {
    const lines = showcaseMarkdown.split("\n");
    const start = lines.findIndex((line) => new RegExp(`^#{1,6}\\s+${caseId}-`, "u").test(line));
    assert.ok(start >= 0, `The canonical corpus is missing ${caseId}`);
    const level = /^#+/u.exec(lines[start])[0].length;
    let end = lines.length;
    for (let index = start + 1; index < lines.length; index++) {
        const heading = /^(#{1,6})\s+/u.exec(lines[index]);
        if (heading && heading[1].length <= level) {
            end = index;
            break;
        }
    }
    return lines.slice(start, end).join("\n").trim();
};

const main = async () => {
    const {call, socket} = await connect();
    const outputDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-markdown-appearance-"));
    const reports = [];
    let coordinateMapping = null;
    let compoundTopology = null;
    let emptyReport = null;
    let nestedQuoteDepths = [];
    let nestedQuoteGeometry = null;
    const showcaseReports = [];
    const tableShowcaseParity = {};
    let verticalRhythm = null;
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
            for (const contractId of ["inline.code", "inline.highlight", "inline.link"]) {
                const measurement = stateReports[0].report.measurements.find((item) => item.contractId === contractId);
                assert.equal(
                    measurement?.styleDiffs.color,
                    undefined,
                    `${contractId} differs from the native theme color: ${JSON.stringify(row)}`,
                );
            }
            if (row.mode === "visual") {
                const equivalentMeasurements = stateReports[0].report.measurements.filter((measurement) =>
                    measurement.markdown && measurement.native);
                for (const measurement of equivalentMeasurements) {
                    assert.deepEqual(
                        measurement.styleDiffs,
                        {},
                        `${measurement.contractId} differs from its native equivalent: ${JSON.stringify(row)}`,
                    );
                }
                const syntaxParity = await evaluate(call, `(() => {
                    const root = document.querySelector(".markdown-appearance-runtime__markdown");
                    const bulletMarkers = Array.from(root?.querySelectorAll(".cm-markra-list-marker--bullet") ?? []);
                    const orderedMarkers = Array.from(root?.querySelectorAll(".cm-markra-list-marker--ordered") ?? []);
                    const structuralLines = Array.from(root?.querySelectorAll(
                        ".cm-markra-structural-line:not(.cm-markra-active-structural-line)",
                    ) ?? []);
                    const nestedLine = root?.querySelector('.cm-markra-list-item[data-list-depth="1"]');
                    const completedTask = root?.querySelector(".cm-markra-task-done");
                    const nativeCompletedTask = document.querySelector(
                        ".markdown-appearance-runtime__native .protyle-task--done > .p",
                    );
                    const completedTaskStyle = completedTask ? getComputedStyle(completedTask) : null;
                    const nativeCompletedTaskStyle = nativeCompletedTask ? getComputedStyle(nativeCompletedTask) : null;
                    const nestedGuideStyle = nestedLine ? getComputedStyle(nestedLine, "::after") : null;
                    const quoteRails = Array.from(root?.querySelectorAll(".cm-markra-blockquote-decoration") ?? []);
                    return {
                        bulletIconHrefs: bulletMarkers.map((marker) => marker.querySelector("use")?.getAttribute("href") ?? ""),
                        bulletMarkerWidths: bulletMarkers.map((marker) => marker.getBoundingClientRect().width),
                        nestedPaddingLeft: nestedLine ? Number.parseFloat(getComputedStyle(nestedLine).paddingLeft) : -1,
                        nestedGuideBackgroundImage: nestedGuideStyle?.backgroundImage ?? "none",
                        nestedGuideWidth: nestedGuideStyle ? Number.parseFloat(nestedGuideStyle.width) : -1,
                        orderedMarkerCount: orderedMarkers.length,
                        orderedMarkerWidths: orderedMarkers.map((marker) => marker.getBoundingClientRect().width),
                        quoteRailHeights: quoteRails.map((rail) => rail.getBoundingClientRect().height),
                        quoteDecorationMode: root?.getAttribute("data-markdown-blockquote-decoration") ?? "",
                        quoteRailWidths: quoteRails.map((rail) => Number.parseFloat(getComputedStyle(rail, "::before").width)),
                        quotedListCount: root?.querySelectorAll(".cm-markra-blockquote.cm-markra-list-item").length ?? 0,
                        setextMarkerCount: root?.querySelectorAll(".cm-markra-setext-marker-line").length ?? 0,
                        structuralHeights: structuralLines.map((line) => line.getBoundingClientRect().height),
                        taskDoneColor: completedTaskStyle?.color ?? "",
                        taskDoneDecoration: completedTaskStyle?.textDecorationLine ?? "",
                        nativeTaskDoneColor: nativeCompletedTaskStyle?.color ?? "",
                        nativeTaskDoneDecoration: nativeCompletedTaskStyle?.textDecorationLine ?? "",
                    };
                })()`);
                assert.ok(syntaxParity.bulletIconHrefs.length >= 2, "Markdown must render unordered-list markers");
                assert.ok(syntaxParity.bulletIconHrefs.every((href) => href === "#iconDot"),
                    "Markdown unordered lists must reuse the native dot icon");
                assert.ok(syntaxParity.bulletMarkerWidths.every((width) => Math.abs(width - 34) <= 1),
                    "Markdown unordered-list markers must keep the native 34px action width");
                assert.ok(syntaxParity.orderedMarkerCount >= 2, "Markdown must render ordered-list markers");
                assert.ok(syntaxParity.orderedMarkerWidths.every((width) => Math.abs(width - 34) <= 1),
                    "Markdown ordered-list markers must keep the native 34px action width");
                assert.ok(syntaxParity.quotedListCount >= 3, "Markdown must preserve list roles inside blockquotes");
                assert.ok(syntaxParity.quoteRailWidths.length >= 1,
                    "Markdown blockquotes must expose structural decorations");
                assert.notEqual(syntaxParity.quoteDecorationMode, "",
                    "Markdown blockquotes must declare the resolved native decoration strategy");
                assert.ok(syntaxParity.quoteRailWidths.every((width) => width > 0),
                    "Markdown blockquote rails must keep the native visible width");
                assert.ok(syntaxParity.quoteRailHeights.every((height) => height > 0),
                    "Markdown blockquote rails must span their semantic containers");
                assert.ok(syntaxParity.nestedPaddingLeft >= 33, "Nested Markdown lists must preserve native indentation");
                assert.notEqual(syntaxParity.nestedGuideBackgroundImage, "none",
                    "Nested Markdown lists must render native hierarchy guides");
                assert.ok(syntaxParity.nestedGuideWidth >= 33,
                    "Nested Markdown hierarchy guides must span the authored nesting depth");
                assert.equal(syntaxParity.taskDoneDecoration, syntaxParity.nativeTaskDoneDecoration,
                    "Completed Markdown tasks must use the native completion decoration");
                assert.equal(syntaxParity.taskDoneColor, syntaxParity.nativeTaskDoneColor,
                    "Completed Markdown tasks must use the native completion color");
                assert.equal(syntaxParity.setextMarkerCount, 0,
                    "Markdown must keep unsupported Setext syntax aligned with the native editor");
                assert.ok(syntaxParity.structuralHeights.length >= 2,
                    "Markdown fixture must expose authored quote structural lines");
                assert.ok(syntaxParity.structuralHeights.every((height) => height <= 1),
                    "Inactive Markdown structural lines must not add visible spacing");
            }
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
                if (state === "media" && row.mode === "visual") {
                    const initialViewer = await evaluate(call, `(() => {
                        const dialog = document.querySelector(".markra-media-viewer-dialog");
                        const panel = dialog?.querySelector(".markra-media-viewer-panel");
                        if (!dialog || !panel) return null;
                        const dialogRect = dialog.getBoundingClientRect();
                        const panelRect = panel.getBoundingClientRect();
                        return {
                            dialogBottom: dialogRect.bottom,
                            dialogLeft: dialogRect.left,
                            dialogPosition: getComputedStyle(dialog).position,
                            dialogRight: dialogRect.right,
                            dialogTop: dialogRect.top,
                            panelHeight: panelRect.height,
                            panelWidth: panelRect.width,
                            viewportHeight: innerHeight,
                            viewportWidth: innerWidth,
                        };
                    })()`);
                    assert.ok(initialViewer, "The media viewer must open from the rendered media control");
                    assert.equal(initialViewer.dialogPosition, "fixed", "The media viewer must be anchored to the viewport");
                    assert.ok(Math.abs(initialViewer.dialogLeft) <= 1 && Math.abs(initialViewer.dialogTop) <= 1,
                        `The media viewer must start at the viewport origin: ${JSON.stringify(initialViewer)}`);
                    assert.ok(
                        Math.abs(initialViewer.dialogRight - initialViewer.viewportWidth) <= 1 &&
                        Math.abs(initialViewer.dialogBottom - initialViewer.viewportHeight) <= 1,
                        `The media viewer must cover the viewport: ${JSON.stringify(initialViewer)}`,
                    );
                    await evaluate(call, `(() => {
                        document.querySelector(".markra-media-viewer-fullscreen-button")?.click();
                        return new Promise((resolve) => requestAnimationFrame(() => resolve(true)));
                    })()`);
                    const fullscreenPanel = await evaluate(call, `(() => {
                        const panel = document.querySelector(".markra-media-viewer-panel");
                        if (!panel) return null;
                        const rect = panel.getBoundingClientRect();
                        return {height: rect.height, left: rect.left, top: rect.top, width: rect.width};
                    })()`);
                    assert.ok(fullscreenPanel, "The fullscreen media panel must remain mounted");
                    assert.ok(
                        Math.abs(fullscreenPanel.left) <= 1 && Math.abs(fullscreenPanel.top) <= 1 &&
                        Math.abs(fullscreenPanel.width - initialViewer.viewportWidth) <= 1 &&
                        Math.abs(fullscreenPanel.height - initialViewer.viewportHeight) <= 1,
                        `The fullscreen media panel must fill the viewport: ${JSON.stringify(fullscreenPanel)}`,
                    );
                    await evaluate(call, "document.querySelector(\".markra-media-viewer-close-button\")?.click()");
                }
            }
            reports.push({...row, behavior, continuity, overflow, stateReports});
            fs.writeFileSync(
                path.join(outputDirectory, `${index}-${row.theme}-${row.mode}-${row.platform}.json`),
                JSON.stringify({row, behavior, continuity, overflow, stateReports}, null, 2),
            );
            await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
        }
        const coordinateMarkdown = `${Array.from({length: 40}, (_, index) =>
            `Paragraph ${index + 1} keeps enough rendered lines above the target.`).join("\n\n")}\n\n---\n\n## Coordinate mapping heading\n\nBody`;
        const coordinatePosition = coordinateMarkdown.indexOf("Coordinate mapping heading") + 4;
        const coordinateOptions = {...matrix[0], markdown: coordinateMarkdown, theme: "savor", width: 900};
        await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(coordinateOptions)}).then(() => true)`);
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.setTheme(\"savor\")");
        coordinateMapping = await evaluate(
            call,
            `window.__siyuanMarkdownAppearanceTest.measureCoordinateMapping(${coordinatePosition})`,
        );
        assert.equal(
            coordinateMapping.hitLineNumber,
            coordinateMapping.targetLineNumber,
            "Rendered Markdown coordinates must resolve to their visual source line",
        );
        assert.ok(
            coordinateMapping.blockTopDifference <= 1,
            `CodeMirror line geometry differs from the rendered line by ${coordinateMapping.blockTopDifference}px`,
        );
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
        for (const caseId of showcaseScreenshotIds) {
            const markdown = showcaseCase(caseId);
            const position = markdown.indexOf(`${caseId}-`);
            const showcaseOptions = {...matrix[0], markdown, theme: "daylight", width: 900};
            await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(showcaseOptions)}).then(() => true)`);
            await evaluate(call, "window.__siyuanMarkdownAppearanceTest.setTheme(\"daylight\")");
            await evaluate(call, `window.__siyuanMarkdownAppearanceTest.measureCoordinateMapping(${position})`);
            showcaseReports.push(await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measure()"));
            if (caseId === "Q04") {
                compoundTopology = await evaluate(
                    call,
                    `window.__siyuanMarkdownAppearanceTest.measureCompoundTopology(${position})`,
                );
                assert.equal(compoundTopology.calloutRailCount, 0, "Q04 must use the normal blockquote rail");
                assert.equal(compoundTopology.quoteRailCount, 1, "Q04 must expose one semantic rail");
                assert.ok(compoundTopology.quoteRailHeight > 0, "Q04 rail must have positive height");
                assert.ok(compoundTopology.quoteRailWidth > 0, "Q04 rail must have positive width");
                assert.ok(compoundTopology.quoteHostWidth >= compoundTopology.quoteLineWidth - 1,
                    `Q04 host must cover the quoted line width: ${JSON.stringify(compoundTopology)}`);
                assert.ok(Math.abs(compoundTopology.quoteHostTopInset - compoundTopology.quoteMarginTop) <= 1,
                    `Q04 host top must exclude the native outer margin: ${JSON.stringify(compoundTopology)}`);
                assert.ok(Math.abs(compoundTopology.quoteHostBottomInset - compoundTopology.quoteMarginBottom) <= 1,
                    `Q04 host bottom must exclude the native outer margin: ${JSON.stringify(compoundTopology)}`);
                assert.equal(compoundTopology.roundedInternalSegmentCount, 0,
                    "Q04 must not retain rounded line-level rail segments");
            }
            if (caseId === "Q05") {
                await evaluate(call, `window.__siyuanMarkdownAppearanceTest.measureCompoundTopology(${position})`);
                nestedQuoteDepths = await evaluate(call, `Array.from(document.querySelectorAll(
                    ".markdown-appearance-runtime__markdown .cm-markra-blockquote-decoration"
                )).map((rail) => rail.style.getPropertyValue("--markra-blockquote-depth"))`);
                assert.ok(nestedQuoteDepths.includes("0") && nestedQuoteDepths.includes("1"),
                    `Q05 must expose distinct outer and inner rails: ${JSON.stringify(nestedQuoteDepths)}`);
                nestedQuoteGeometry = await evaluate(call, `(() => {
                    const root = document.querySelector(".markdown-appearance-runtime");
                    const native = Array.from(root?.querySelectorAll(".markdown-appearance-runtime__native .bq") ?? []);
                    const markdown = Array.from(root?.querySelectorAll(
                        ".markdown-appearance-runtime__markdown .cm-markra-blockquote-decoration"
                    ) ?? []).sort((left, right) => Number(left.style.getPropertyValue("--markra-blockquote-depth")) -
                        Number(right.style.getPropertyValue("--markra-blockquote-depth")));
                    if (native.length !== 2 || markdown.length !== 2) return null;
                    const nativeOuter = native[0].getBoundingClientRect();
                    const nativeInner = native[1].getBoundingClientRect();
                    const markdownOuter = markdown[0].getBoundingClientRect();
                    const markdownInner = markdown[1].getBoundingClientRect();
                    return {
                        innerHeightDifference: Math.abs(nativeInner.height - markdownInner.height),
                        leftInsetDifference: Math.abs(
                            nativeInner.left - nativeOuter.left - (markdownInner.left - markdownOuter.left)
                        ),
                        outerHeightDifference: Math.abs(nativeOuter.height - markdownOuter.height),
                        rightInsetDifference: Math.abs(
                            nativeOuter.right - nativeInner.right - (markdownOuter.right - markdownInner.right)
                        ),
                    };
                })()`);
                assert.ok(nestedQuoteGeometry, "Q05 nested quote geometry must be measurable");
                assert.ok(nestedQuoteGeometry.outerHeightDifference <= 1,
                    `Q05 outer quote height must match native: ${JSON.stringify(nestedQuoteGeometry)}`);
                assert.ok(nestedQuoteGeometry.innerHeightDifference <= 1,
                    `Q05 inner quote height must match native: ${JSON.stringify(nestedQuoteGeometry)}`);
                assert.ok(nestedQuoteGeometry.leftInsetDifference <= 1 && nestedQuoteGeometry.rightInsetDifference <= 1,
                    `Q05 inner quote insets must match native: ${JSON.stringify(nestedQuoteGeometry)}`);
            }
            if (["T03", "T04", "T05", "T06"].includes(caseId)) {
                tableShowcaseParity[caseId] = await evaluate(call, `(() => {
                    const root = document.querySelector(".markdown-appearance-runtime");
                    const nativeTable = root?.querySelector(".markdown-appearance-runtime__native .table table");
                    const markdownTable = root?.querySelector(".markdown-appearance-runtime__markdown .cm-markra-table");
                    const markdownOuter = root?.querySelector(
                        ".markdown-appearance-runtime__markdown .markra-table-scroll"
                    );
                    if (!nativeTable || !markdownTable || !markdownOuter) return null;
                    const nativeStyle = getComputedStyle(nativeTable);
                    const markdownStyle = getComputedStyle(markdownTable);
                    const markdownOuterStyle = getComputedStyle(markdownOuter);
                    const nativeLink = nativeTable.querySelector('[data-type~="a"]');
                    const markdownLink = markdownTable.querySelector("a");
                    const nativeLinkStyle = nativeLink ? getComputedStyle(nativeLink) : null;
                    const markdownLinkStyle = markdownLink ? getComputedStyle(markdownLink) : null;
                    return {
                        display: [nativeStyle.display, markdownOuterStyle.display],
                        inlineCode: [nativeTable.querySelector('[data-type~="code"]')?.textContent ?? "",
                            markdownTable.querySelector("code")?.textContent ?? ""].map((value) =>
                            value.replace(/[\u200b\u200c\u200d\ufeff]/gu, "").trim()),
                        linkClass: markdownLink?.classList.contains("cm-markra-link") ?? false,
                        linkColor: [nativeLinkStyle?.color ?? "", markdownLinkStyle?.color ?? ""],
                        linkDecoration: [nativeLinkStyle?.textDecorationLine ?? "", markdownLinkStyle?.textDecorationLine ?? ""],
                        linkDecorationColor: [nativeLinkStyle?.textDecorationColor ?? "",
                            markdownLinkStyle?.textDecorationColor ?? ""],
                        overflow: [nativeStyle.overflow, markdownOuterStyle.overflow],
                        radius: [nativeStyle.borderRadius, markdownOuterStyle.borderRadius],
                        semanticDisplay: markdownStyle.display,
                        widthDifference: Math.abs(nativeTable.getBoundingClientRect().width -
                            markdownTable.getBoundingClientRect().width),
                    };
                })()`);
                assert.ok(tableShowcaseParity[caseId], `${caseId} table parity must be measurable`);
                assert.equal(tableShowcaseParity[caseId].display[0], tableShowcaseParity[caseId].display[1],
                    `${caseId} table display topology must match native`);
                assert.equal(tableShowcaseParity[caseId].overflow[0], tableShowcaseParity[caseId].overflow[1],
                    `${caseId} table clipping topology must match native`);
                assert.equal(tableShowcaseParity[caseId].radius[0], tableShowcaseParity[caseId].radius[1],
                    `${caseId} table radius must match native`);
                assert.equal(tableShowcaseParity[caseId].semanticDisplay, "table",
                    `${caseId} semantic table must preserve the table layout algorithm`);
                assert.ok(tableShowcaseParity[caseId].widthDifference <= 1,
                    `${caseId} table width must match native: ${JSON.stringify(tableShowcaseParity[caseId])}`);
                if (caseId === "T03") {
                    assert.deepEqual(tableShowcaseParity[caseId].inlineCode, ["a | b", "a | b"],
                        `T03 escaped pipe display must match native: ${JSON.stringify(tableShowcaseParity[caseId])}`);
                }
                if (caseId === "T05") {
                    assert.equal(tableShowcaseParity[caseId].linkClass, true,
                        "T05 table links must use the shared Markdown link appearance contract");
                    assert.equal(tableShowcaseParity[caseId].linkColor[0], tableShowcaseParity[caseId].linkColor[1]);
                    assert.equal(tableShowcaseParity[caseId].linkDecoration[0],
                        tableShowcaseParity[caseId].linkDecoration[1]);
                    assert.equal(tableShowcaseParity[caseId].linkDecorationColor[0],
                        tableShowcaseParity[caseId].linkDecorationColor[1]);
                }
            }
            if (caseId === "M03") {
                const mathLayout = await evaluate(call, `(() => {
                    const block = document.querySelector(".markdown-appearance-runtime__markdown .markra-math-render-display");
                    const katex = block?.querySelector(".katex");
                    const blockRect = block?.getBoundingClientRect();
                    const katexRect = katex?.getBoundingClientRect();
                    const style = block ? getComputedStyle(block) : null;
                    return {
                        borderBottomWidth: Number.parseFloat(style?.borderBottomWidth ?? "-1"),
                        borderLeftWidth: Number.parseFloat(style?.borderLeftWidth ?? "-1"),
                        borderRightWidth: Number.parseFloat(style?.borderRightWidth ?? "-1"),
                        borderTopWidth: Number.parseFloat(style?.borderTopWidth ?? "-1"),
                        centerDifference: blockRect && katexRect
                            ? Math.abs((blockRect.left + blockRect.right - katexRect.left - katexRect.right) / 2)
                            : Number.POSITIVE_INFINITY,
                        spacerCount: block?.querySelectorAll(".katex-html > .fn__flex-1").length ?? 0,
                    };
                })()`);
                assert.deepEqual(
                    [mathLayout.borderTopWidth, mathLayout.borderRightWidth,
                        mathLayout.borderBottomWidth, mathLayout.borderLeftWidth],
                    [0, 0, 0, 0],
                    "Markdown display math must not expose an outer border",
                );
                assert.ok(mathLayout.centerDifference <= 1,
                    `Markdown display math differs from the block center by ${mathLayout.centerDifference}px`);
                assert.equal(mathLayout.spacerCount, 1,
                    "Markdown display math must use the native KaTeX balancing spacer");
            }
            if (caseId === "T01") {
                const tableWidths = await evaluate(call, `(async () => {
                    const root = document.querySelector(".markdown-appearance-runtime__markdown");
                    const table = root?.querySelector(".cm-markra-table");
                    const scroll = root?.querySelector(".markra-table-scroll");
                    const button = root?.querySelector(".markra-table-width-button");
                    if (!table || !scroll || !button) throw new Error("T01 table width controls are incomplete");
                    const measure = () => {
                        const currentTable = root.querySelector(".cm-markra-table");
                        const currentScroll = root.querySelector(".markra-table-scroll");
                        if (!currentTable || !currentScroll) throw new Error("T01 table was removed during width change");
                        return {
                            cellWidths: Array.from(currentTable.querySelectorAll("thead th"), (cell) =>
                                cell.getBoundingClientRect().width),
                            mode: currentTable.dataset.widthMode,
                            scrollDisplay: getComputedStyle(currentScroll).display,
                            tableDisplay: getComputedStyle(currentTable).display,
                            tableLayout: getComputedStyle(currentTable).tableLayout,
                            tableWidth: currentTable.getBoundingClientRect().width,
                            viewportWidth: currentScroll.getBoundingClientRect().width,
                        };
                    };
                    const auto = measure();
                    button.click();
                    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
                    const even = measure();
                    return {auto, even};
                })()`);
                assert.equal(tableWidths.auto.mode, "auto");
                assert.equal(tableWidths.auto.tableDisplay, "table");
                assert.equal(tableWidths.auto.tableLayout, "auto");
                assert.ok(Math.abs(tableWidths.auto.cellWidths[0] - tableWidths.auto.cellWidths[1]) > 1,
                    `T01 auto width must follow cell content: ${JSON.stringify(tableWidths)}`);
                assert.ok(tableWidths.auto.tableWidth <= tableWidths.auto.viewportWidth,
                    `T01 auto width must keep native content sizing: ${JSON.stringify(tableWidths)}`);
                assert.equal(tableWidths.even.mode, "even");
                assert.equal(tableWidths.even.tableDisplay, "table");
                assert.equal(tableWidths.even.tableLayout, "fixed");
                assert.ok(Math.abs(tableWidths.even.cellWidths[0] - tableWidths.even.cellWidths[1]) <= 1,
                    `T01 even width must distribute columns evenly: ${JSON.stringify(tableWidths)}`);
                assert.ok(Math.abs(tableWidths.even.tableWidth - tableWidths.even.viewportWidth) <= 1,
                    `T01 even width must fill its viewport: ${JSON.stringify(tableWidths)}`);
            }
            const screenshot = await call("Page.captureScreenshot", {format: "png", fromSurface: true});
            fs.writeFileSync(path.join(outputDirectory, `showcase-${caseId}.png`), screenshot.data, "base64");
            await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
        }
        const rhythmMarkdown = `## 7.5 工作量

Android 端剩余工作量（从 v1.5 估算）≈ 15.5 人日。

---

## §8 iOS 实现要点

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v1.7 |
| 作者 | 研发架构组 |

iOS 部分目前项目中不存在成熟推送实现。

### 8.1 通道选型

- 方案一
- 方案二

> 引用说明`;
        const rhythmOptions = {...matrix[0], markdown: rhythmMarkdown, theme: "savor", width: 900};
        await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(rhythmOptions)}).then(() => true)`);
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.setTheme(\"savor\")");
        verticalRhythm = await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measureVerticalRhythm()");
        assert.equal(verticalRhythm.kindsMatch, true, "Native and Markdown block sequences must match");
        assert.ok(
            verticalRhythm.maximumTopDifference <= 2,
            `Native and Markdown block positions differ by ${verticalRhythm.maximumTopDifference}px`,
        );
        assert.ok(
            verticalRhythm.totalHeightDifference <= 2,
            `Native and Markdown block sequence heights differ by ${verticalRhythm.totalHeightDifference}px`,
        );
        const tableLayout = await evaluate(call, `(async () => {
            const root = document.querySelector(".markdown-appearance-runtime");
            const nativeHeading = root?.querySelector(".markdown-appearance-runtime__native .h2");
            const nativeTable = root?.querySelector(".markdown-appearance-runtime__native .table");
            const markdownHeading = root?.querySelector(".markdown-appearance-runtime__markdown .cm-markra-h2");
            const markdownTable = root?.querySelector(".markdown-appearance-runtime__markdown .cm-markra-table-wrap");
            const firstHeader = markdownTable?.querySelector("th");
            firstHeader?.focus();
            await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
            const controls = markdownTable?.querySelector(".markra-table-align-controls");
            if (!nativeHeading || !nativeTable || !markdownHeading || !markdownTable || !firstHeader || !controls) {
                return null;
            }
            const nativeTableRect = nativeTable.getBoundingClientRect();
            const markdownTableRect = markdownTable.getBoundingClientRect();
            const controlsRect = controls.getBoundingClientRect();
            const headerRect = firstHeader.getBoundingClientRect();
            const headerHitTarget = document.elementFromPoint(
                headerRect.left + headerRect.width / 2,
                headerRect.top + headerRect.height / 2,
            );
            return {
                controlsOutsideTable: controlsRect.bottom <= markdownTableRect.top ||
                    controlsRect.top >= markdownTableRect.bottom,
                firstHeaderHitTarget: firstHeader === headerHitTarget || firstHeader.contains(headerHitTarget),
                headerOverlap: Math.max(0, Math.min(controlsRect.bottom, headerRect.bottom) -
                    Math.max(controlsRect.top, headerRect.top)),
                tableTopDifference: Math.abs(nativeTableRect.top - markdownTableRect.top),
            };
        })()`);
        assert.ok(tableLayout, "Markdown table layout measurements must be available");
        assert.equal(tableLayout.controlsOutsideTable, true, "Markdown table controls must remain outside the table");
        assert.equal(tableLayout.firstHeaderHitTarget, true, "Markdown table first header must remain pointer-accessible");
        assert.ok(tableLayout.headerOverlap <= 1, "Markdown table controls must not overlap the first row");
        assert.ok(
            tableLayout.tableTopDifference <= 2,
            `Markdown table top differs from native by ${tableLayout.tableTopDifference}px`,
        );
        const dividerPaint = await evaluate(call, `(() => {
            const rule = document.querySelector(".markdown-appearance-runtime__markdown .cm-markra-horizontal-rule");
            const style = rule ? getComputedStyle(rule, "::after") : null;
            return {background: style?.background ?? "", height: Number.parseFloat(style?.height ?? "0")};
        })()`);
        assert.ok(dividerPaint.height > 0, "Markdown horizontal rule must expose a visible paint layer");
        assert.doesNotMatch(dividerPaint.background, /^(?:|none|rgba\(0, 0, 0, 0\))$/u);
        const rhythmScreenshot = await call("Page.captureScreenshot", {format: "png", fromSurface: true});
        fs.writeFileSync(path.join(outputDirectory, "savor-vertical-rhythm.png"), rhythmScreenshot.data, "base64");
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
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
        ...showcaseReports,
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
    const intentionallyUnrenderedContracts = ["editor.gutter"];
    assert.equal(matrix.length, 7);
    assert.ok(contractCount >= 50, `Unexpected appearance contract count: ${contractCount}`);
    assert.deepEqual(missingRequired, [], `Required runtime contracts were not rendered: ${missingRequired.join(", ")}`);
    const knownContracts = new Set(allReports.flatMap((report) => report.measurements.map((measurement) => measurement.contractId)));
    const summary = {
        contractCount,
        compoundTopology,
        coordinateMapping,
        matrixRows: reports.length,
        nestedQuoteDepths,
        nestedQuoteGeometry,
        outputDirectory,
        screenshotCount: fs.readdirSync(outputDirectory).filter((name) => name.endsWith(".png")).length,
        seenContracts: seen.size,
        tableShowcaseParity,
        uncovered: [...knownContracts].filter((id) => !seen.has(id)),
        verticalRhythm,
    };
    fs.writeFileSync(path.join(outputDirectory, "summary.json"), JSON.stringify(summary, null, 2));
    assert.equal(summary.screenshotCount, matrix.length + 2 + showcaseScreenshotIds.length);
    assert.deepEqual(
        summary.uncovered.sort(),
        intentionallyUnrenderedContracts.sort(),
        `Unexpected appearance contracts not rendered: ${summary.uncovered.join(", ")}`,
    );
    console.log("Markdown appearance runtime matrix passed", summary);
};

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
