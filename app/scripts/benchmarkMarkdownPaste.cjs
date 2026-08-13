const {markdownToBlockDOM} = require("./markdownAppearanceFixture.cjs");

markdownToBlockDOM("");
const Lute = global.Lute || global.window?.Lute;
if (!Lute?.New || !Lute?.Sanitize) {
    throw new Error("The bundled Lute runtime is unavailable");
}

const richParagraph = '<p style="white-space:pre-wrap"><strong>QingYu</strong> clipboard paragraph with <a href="https://example.com">a link</a>.</p>';
const samples = [
    {
        html: richParagraph.repeat(420),
        name: "rich-text-50kb",
    },
    {
        html: `<table>${Array.from({length: 200}, (_, row) => `<tr><td>Row ${row + 1}</td><td>Value ${row + 1}</td><td>说明 ${row + 1}</td></tr>`).join("")}</table>`,
        name: "table-200-rows",
    },
    {
        html: `<pre><code class="language-typescript">${Array.from({length: 2000}, (_, line) => `const value${line + 1} = ${line + 1};`).join("\n")}</code></pre>`,
        name: "code-2000-lines",
    },
    {
        html: `<!doctype html><html><body>${richParagraph.repeat(8200)}</body></html>`,
        name: "complete-page-1mb",
    },
];

const lute = Lute.New();
lute.SetUnorderedListMarker("-");

const convert = (html) => lute.HTML2Md(Lute.Sanitize(html));
for (const sample of samples) convert(sample.html);

const report = samples.map(({name, html}) => {
    const timings = Array.from({length: 5}, () => {
        const start = performance.now();
        const markdown = convert(html);
        if (!markdown) throw new Error(`${name} produced empty Markdown`);
        return performance.now() - start;
    }).sort((a, b) => a - b);
    return {
        inputBytes: new TextEncoder().encode(html).length,
        iterations: timings.length,
        maximumMs: Number(timings.at(-1).toFixed(3)),
        medianMs: Number(timings[Math.floor(timings.length / 2)].toFixed(3)),
        name,
    };
});

console.log(JSON.stringify(report, null, 2));
