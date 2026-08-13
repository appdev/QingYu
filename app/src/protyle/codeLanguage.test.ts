import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markdown/markraTestDom";
import {getCodeLanguages} from "./codeLanguage";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => cleanup());

test("provides every bundled code language before Highlight.js loads", () => {
    const highlight = require("../../stage/protyle/js/highlight.js/highlight.min.js") as {
        listLanguages(): string[];
    };
    const thirdLanguageSource = readFileSync(
        resolve(process.cwd(), "stage/protyle/js/highlight.js/third-languages.js"),
        "utf8",
    );
    const thirdLanguages = Array.from(
        thirdLanguageSource.matchAll(/registerLanguage\((["'])([^"']+)\1/gu),
        (match) => match[2],
    );
    const expected = [...new Set([
        "js", "ts", "html", "toml", "c#", "bat",
        ...highlight.listLanguages(),
        ...thirdLanguages,
    ])].sort();
    Object.assign(window, {hljs: undefined});

    assert.deepEqual(getCodeLanguages(), expected);
});
