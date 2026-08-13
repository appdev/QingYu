import assert = require("node:assert/strict");
import {before, test} from "node:test";
import {unified} from "unified";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import {convertCodeMirrorClipboardHtml} from "./markra-core/codemirror";
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
