#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");

const appRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(appRoot, "..");
const locales = ["zh-CN", "zh-TW", "en", "ja"];
const kinds = ["privacy", "terms"];
const approvedOrigins = new Set(["https://apkdv.com", "https://github.com"]);

const escapeHTML = (value) => value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;").replaceAll("'", "&#39;");

const validateLink = (url) => {
    if (url.startsWith("/") || url === "mailto:lengyue@apkdv.com") {
        return url;
    }
    const parsed = new URL(url);
    if (!approvedOrigins.has(parsed.origin)) {
        throw new Error(`unapproved legal-document link ${url}`);
    }
    if (parsed.origin === "https://github.com" &&
        !["/appdev", "/appdev/QingYu"].some((prefix) => parsed.pathname === prefix || parsed.pathname.startsWith(`${prefix}/`))) {
        throw new Error(`unapproved GitHub legal-document link ${url}`);
    }
    return url;
};

const renderMarkdown = async (source, locale) => {
    const [{unified}, {default: remarkParse}] = await Promise.all([
        import("unified"), import("remark-parse"),
    ]);
    const tree = unified().use(remarkParse).parse(source);
    let documentTitle = "QingYu";
    const renderChildren = (node) => (node.children || []).map(renderNode).join("");
    const renderNode = (node) => {
        switch (node.type) {
            case "root": return renderChildren(node);
            case "text": return escapeHTML(node.value);
            case "heading": {
                const content = renderChildren(node);
                if (node.depth === 1) {
                    documentTitle = (node.children || []).map((child) => child.value || "").join("");
                }
                return `<h${node.depth}>${content}</h${node.depth}>`;
            }
            case "paragraph": return `<p>${renderChildren(node)}</p>`;
            case "strong": return `<strong>${renderChildren(node)}</strong>`;
            case "emphasis": return `<em>${renderChildren(node)}</em>`;
            case "inlineCode": return `<code>${escapeHTML(node.value)}</code>`;
            case "link": return `<a href="${escapeHTML(validateLink(node.url))}" rel="noreferrer">${renderChildren(node)}</a>`;
            case "list": return `<${node.ordered ? "ol" : "ul"}>${renderChildren(node)}</${node.ordered ? "ol" : "ul"}>`;
            case "listItem": return `<li>${renderChildren(node)}</li>`;
            case "break": return "<br>";
            case "thematicBreak": return "<hr>";
            default: throw new Error(`unsupported legal Markdown node ${node.type}`);
        }
    };
    const body = renderNode(tree);
    return `<!doctype html>
<html lang="${escapeHTML(locale)}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src 'self' data:">
<title>${escapeHTML(documentTitle)}</title>
<style>body{box-sizing:border-box;max-width:840px;margin:0 auto;padding:32px 24px 64px;color:#202124;background:#fff;font:16px/1.7 system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}h1{font-size:2rem;line-height:1.25}h2{margin-top:2rem;font-size:1.35rem}a{color:#315efb}code{padding:.1em .3em;background:#f1f3f4;border-radius:4px}@media(prefers-color-scheme:dark){body{color:#e8eaed;background:#202124}a{color:#8ab4f8}code{background:#303134}}</style>
</head>
<body>${body}</body>
</html>
`;
};

const buildOutputs = async () => {
    const outputs = new Map();
    for (const kind of kinds) {
        for (const locale of locales) {
            const sourcePath = path.join(repositoryRoot, "docs", "legal", `${kind}.${locale}.md`);
            outputs.set(`${kind}.${locale}.html`, await renderMarkdown(fs.readFileSync(sourcePath, "utf8"), locale));
        }
    }
    return outputs;
};

const writeOutputs = async () => {
    const outputDir = path.join(appRoot, "stage", "legal");
    fs.mkdirSync(outputDir, {recursive: true});
    for (const [name, content] of await buildOutputs()) {
        fs.writeFileSync(path.join(outputDir, name), content);
    }
};

const checkOutputs = async () => {
    const outputDir = path.join(appRoot, "stage", "legal");
    const errors = [];
    const outputs = await buildOutputs();
    for (const [name, content] of outputs) {
        const target = path.join(outputDir, name);
        if (!fs.existsSync(target) || fs.readFileSync(target, "utf8") !== content) {
            errors.push(name);
        }
    }
    if (errors.length > 0) {
        throw new Error(`legal HTML differs from source: ${errors.join(", ")}`);
    }
};

if (require.main === module) {
    const action = process.argv.includes("--write") ? writeOutputs : process.argv.includes("--check") ? checkOutputs : null;
    if (!action) {
        throw new Error("use --write or --check");
    }
    action().catch((error) => {
        process.stderr.write(`${error.stack || error}\n`);
        process.exitCode = 1;
    });
}

module.exports = {renderMarkdown, validateLink};
