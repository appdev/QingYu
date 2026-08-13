import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import TurndownService = require("turndown");
import {EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {convertCodeMirrorClipboardHtml} from "./markra-core/codemirror";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";

let cleanup: () => void;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

test("prefers the host HTML to Markdown converter for structured clipboard content", () => {
    let received = "";
    const result = convertCodeMirrorClipboardHtml(
        "<h1>标题</h1><table><tr><th>项目</th></tr><tr><td>内容</td></tr></table>",
        "标题\n项目\n内容",
        (html) => {
            received = html;
            return "# 标题\n\n| 项目 |\n| --- |\n| 内容 |";
        },
    );

    assert.ok(result);
    assert.match(received, /<h1>标题<\/h1>/u);
    assert.equal(result.markdown, "# 标题\n\n| 项目 |\n| --- |\n| 内容 |");
});

test("falls back to Turndown when the host converter returns no Markdown", () => {
    const result = convertCodeMirrorClipboardHtml("<h2>回退标题</h2>", "", () => "");

    assert.equal(result?.markdown, "## 回退标题");
});

test("falls back to Turndown when the host converter throws", () => {
    const result = convertCodeMirrorClipboardHtml("<strong>粗体</strong>", "", () => {
        throw new Error("converter unavailable");
    });

    assert.equal(result?.markdown, "**粗体**");
});

test("creates the Turndown fallback from a webpack default export", async () => {
    const htmlPasteModule = await import("./markra-core/codemirror/html-paste");
    const createClipboardTurndownService = (
        htmlPasteModule as unknown as {
            createClipboardTurndownService?: (module: unknown) => TurndownService;
        }
    ).createClipboardTurndownService;
    assert.equal(typeof createClipboardTurndownService, "function");
    assert.equal(
        createClipboardTurndownService?.({default: TurndownService}).turndown("<h1>标题</h1>"),
        "# 标题",
    );
});

test("uses the host converter through the CodeMirror paste pipeline", () => {
    const expected = "# Lute：Flutter 与三端局域网扫描协议\n\n| 项目 | 内容 |\n| --- | --- |\n| 协议名称 | Bridge 协议 |";
    const adapter: MarkdownHostAdapter = {
        convertHtmlToMarkdown: () => expected,
        createIcon: (_name, className, ownerDocument) => {
            const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
            icon.classList.add(className);
            return icon;
        },
        notifyError() {},
        openLink() {},
        positionPopover() {},
        renderMath: (_source, _displayMode, context) => context.ownerDocument.createElement("span"),
        renderMermaid: async (_source, context) => context.ownerDocument.createElement("div"),
        resolveImageSource: (source) => source,
        saveClipboardAssets: async () => [],
    };
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            extensions: [
                minimalSetup,
                createSiyuanMarkraExtension({
                    adapter,
                    documentPath: () => "/test.md",
                    mode: "visual",
                }),
            ],
        }),
    });
    const event = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {
        value: {
            getData(type: string) {
                if (type === "text/html") {
                    return "<h1>Flutter 与三端局域网扫描协议</h1><table><tr><th>项目</th><th>内容</th></tr><tr><td>协议名称</td><td>Bridge 协议</td></tr></table>";
                }
                return type === "text/plain" ? "# Flutter 与三端局域网扫描协议" : "";
            },
        },
    });

    view.contentDOM.dispatchEvent(event);

    assert.equal(view.state.doc.toString(), expected);
});

test("sanitizes HTML and lazily reuses a Lute converter with stable list markers", async () => {
    const previousLute = Object.getOwnPropertyDescriptor(globalThis, "Lute");
    const calls: string[][] = [];
    Object.defineProperty(globalThis, "Lute", {
        configurable: true,
        value: {
            New() {
                calls.push(["new"]);
                return {
                    HTML2Md(html: string) {
                        calls.push(["html2md", html]);
                        return "# 标题\n";
                    },
                    SetUnorderedListMarker(marker: string) {
                        calls.push(["list-marker", marker]);
                    },
                };
            },
            Sanitize(html: string) {
                calls.push(["sanitize", html]);
                return html.replace(" onclick=\"x\"", "");
            },
        },
        writable: true,
    });

    try {
        const {convertSiyuanClipboardHtmlToMarkdown} = await import("./luteHtmlConverter");

        assert.equal(convertSiyuanClipboardHtmlToMarkdown("<h1 onclick=\"x\">标题</h1>"), "# 标题\n");
        assert.equal(convertSiyuanClipboardHtmlToMarkdown("<h1>第二次</h1>"), "# 标题\n");
        assert.deepEqual(calls, [
            ["new"],
            ["list-marker", "-"],
            ["sanitize", "<h1 onclick=\"x\">标题</h1>"],
            ["html2md", "<h1>标题</h1>"],
            ["sanitize", "<h1>第二次</h1>"],
            ["html2md", "<h1>第二次</h1>"],
        ]);
    } finally {
        if (previousLute) {
            Object.defineProperty(globalThis, "Lute", previousLute);
        } else {
            delete (globalThis as Record<string, unknown>).Lute;
        }
    }
});
