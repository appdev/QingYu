import assert = require("node:assert/strict");
import {test} from "node:test";
import {markdownExportFormats} from "./menu";

test("Markdown export capability matrix matches the native platform boundary", () => {
    assert.deepEqual(markdownExportFormats("electron"), [
        "template", "markdownZip", "image", "pdf", "html", "docx", "rst", "asciidoc", "textile", "opml", "org",
        "mediawiki", "odt", "rtf", "epub",
    ]);
    assert.deepEqual(markdownExportFormats("browser"), ["template", "markdownZip", "image", "html"]);
    assert.deepEqual(markdownExportFormats("mobile"), ["template", "markdownZip", "image", "pdf", "html"]);
});
