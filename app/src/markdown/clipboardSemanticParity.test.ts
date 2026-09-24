import assert = require("node:assert/strict");
import {before, test} from "node:test";
import {unified} from "unified";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import {EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {convertCodeMirrorClipboardHtml} from "./markra-core/codemirror";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {installMarkdownTestDom} from "./markraTestDom";

const {markdownToBlockDOM} = require("../../scripts/markdownAppearanceFixture.cjs") as {
    markdownToBlockDOM(markdown: string): string;
};

let convertSiyuanClipboardHtmlToMarkdown: (html: string) => string;

before(async () => {
    markdownToBlockDOM("");
    ({convertSiyuanClipboardHtmlToMarkdown} = await import("./luteHtmlConverter"));
});

const semanticTree = (markdown: string) => {
    const tree = unified()
        .use(remarkParse)
        .use(remarkGfm)
        .use(remarkFrontmatter, ["yaml"])
        .use(remarkMath)
        .parse(markdown);
    return JSON.parse(JSON.stringify(tree, (key, value) => key === "position" ? undefined : value));
};

test("pastes Markdown source wrapped in HTML without escaping code or splitting table rows", () => {
    const cleanup = installMarkdownTestDom();
    const source = "## 验证\n\n使用 `login()`\n\n| 场景 | 预期 |\n|---|---|\n| 登录 | 成功 |\n\n```mermaid\nflowchart TD\n  A --> B\n```\n";
    const html = source.trimEnd().split("\n").filter(Boolean).map((line, index) => {
        const element = document.createElement(index === 0 ? "h2" : "p");
        element.textContent = index === 0 ? "验证" : line;
        return element.outerHTML;
    }).join("");
    let view: EditorView | undefined;
    try {
        view = new EditorView({parent: document.body, state: EditorState.create({extensions: [minimalSetup,
            createSiyuanMarkraExtension({mode: "visual", documentPath: () => "/paste.md", adapter: {
                convertHtmlToMarkdown: convertSiyuanClipboardHtmlToMarkdown,
                createIcon: (_name, _className, ownerDocument) => ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg"),
                notifyError() {}, openLink() {}, positionPopover() {},
                renderMath: (_source, _display, context) => context.ownerDocument.createElement("span"),
                renderMermaid: async (_source, context) => context.ownerDocument.createElement("div"),
                resolveImageSource: (value) => value,
                saveClipboardAssets: async () => [],
            }}),
        ]})});
        const event = new Event("paste", {bubbles: true, cancelable: true});
        Object.defineProperty(event, "clipboardData", {value: {
            getData: (type: string) => type === "text/html" ? html : type === "text/plain" ? source : "",
        }});
        view.contentDOM.dispatchEvent(event);
        assert.equal(view.state.doc.toString(), source);
        assert.ok(view.dom.querySelector("table"));
        assert.ok(view.dom.querySelector(".cm-markra-inline-code"));
        assert.deepEqual(semanticTree(view.state.doc.toString()), semanticTree(source));

        const fallback = convertCodeMirrorClipboardHtml(html, source);
        assert.equal(fallback?.markdown, source);
    } finally {
        view?.destroy();
        cleanup();
    }
});

test("does not discard rich formatting or repair already escaped source", () => {
    const cleanup = installMarkdownTestDom();
    try {
        const html = '<h2>验证</h2><p>使用 `login()`，<a href="https://example.com">链接</a>和<strong>重点</strong></p>';
        const result = convertCodeMirrorClipboardHtml(html, "## 验证\n\n使用 `login()`，链接和重点", convertSiyuanClipboardHtmlToMarkdown);
        assert.equal(result?.source, "host");
        assert.match(result.markdown, /\[链接\]\(https:\/\/example.com\)/u);
        assert.match(result.markdown, /\*\*重点\*\*/u);

        const image = convertCodeMirrorClipboardHtml('<img src="https://example.com/image.png" alt="示例">',
            "![示例](https://example.com/image.png)", convertSiyuanClipboardHtmlToMarkdown);
        assert.equal(image?.markdown, "![示例](https://example.com/image.png)");
        assert.equal(image.remoteImages[0]?.src, "https://example.com/image.png");

        const source = "## 验证\n\n使用 \\`login()\\`\n\n| 场景 | 预期 |\n\n|---|---|";
        const wrapper = document.createElement("div");
        wrapper.textContent = source;
        const escaped = convertCodeMirrorClipboardHtml(wrapper.outerHTML, source, convertSiyuanClipboardHtmlToMarkdown);
        assert.doesNotMatch(JSON.stringify(semanticTree(escaped.markdown)), /"type":"(?:inlineCode|table)"/u);
    } finally {
        cleanup();
    }
});

const fixtures = [
    {
        html: '<p style="white-space: pre-wrap">说明：“本地有效”必然要求 H=true。</p>',
        name: "pre-wrap paragraph",
        plainText: "说明：“本地有效”必然要求 H=true。",
    },
    {
        html: '<table><tr><th>动作码</th><th>含义</th></tr><tr><td>Local</td><td>本地有效</td></tr></table><p style="white-space: pre-wrap">说明：“本地有效”必然要求 H=true。</p>',
        name: "table followed by a styled paragraph",
        plainText: "动作码 含义 Local 本地有效 说明",
    },
    {
        html: '<pre><code class="language-typescript">const enabled = true;</code></pre>',
        name: "explicit code block",
        plainText: "const enabled = true;",
    },
    {
        html: '<blockquote><p>引用 <a href="https://example.com">链接</a></p></blockquote><ul><li>第一项<ul><li>子项</li></ul></li></ul>',
        name: "quote link and nested list",
        plainText: "引用 链接 第一项 子项",
    },
    {
        html: '<p>远程图片 <img src="https://example.com/image.png" alt="示例"></p>',
        name: "remote image",
        plainText: "远程图片 示例",
    },
] as const;

for (const fixture of fixtures) {
    test(`matches native Lute semantics for ${fixture.name}`, () => {
        const cleanup = installMarkdownTestDom();
        try {
            const native = convertSiyuanClipboardHtmlToMarkdown(fixture.html);
            const markdown = convertCodeMirrorClipboardHtml(
                fixture.html,
                fixture.plainText,
                convertSiyuanClipboardHtmlToMarkdown,
            );

            assert.ok(markdown);
            assert.equal(markdown.source, "host");
            assert.deepEqual(semanticTree(markdown.markdown), semanticTree(native));
        } finally {
            cleanup();
        }
    });
}
