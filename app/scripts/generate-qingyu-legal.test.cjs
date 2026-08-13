const assert = require("node:assert/strict");
const test = require("node:test");

const {renderMarkdown, validateLink} = require("./generate-qingyu-legal.cjs");

test("renders the supported legal Markdown subset with a restrictive CSP", async () => {
    const html = await renderMarkdown("# Policy\n\n## Local data\n\nText with **strong** and [source](https://github.com/appdev/QingYu).\n\n- One\n- Two", "en");
    assert.match(html, /default-src 'none'/);
    assert.match(html, /<h1>Policy<\/h1>/);
    assert.match(html, /<strong>strong<\/strong>/);
    assert.match(html, /href="https:\/\/github\.com\/appdev\/QingYu"/);
    assert.match(html, /<ul>/);
});

test("escapes raw markup and rejects unapproved link protocols", async () => {
    const html = await renderMarkdown("# Policy\n\n&lt;script&gt;alert(1)&lt;/script&gt;", "en");
    assert.doesNotMatch(html, /<script>alert/);
    assert.throws(() => validateLink("javascript:alert(1)"));
    assert.throws(() => validateLink("https://example.com/privacy"));
    assert.doesNotThrow(() => validateLink("mailto:lengyue@apkdv.com"));
});
