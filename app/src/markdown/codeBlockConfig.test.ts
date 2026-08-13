import assert = require("node:assert/strict");
import {test} from "node:test";
import {readSiyuanCodeBlockConfig} from "./codeBlockConfig";

test("maps disabled native line numbers without changing wrap or ligatures", () => {
    assert.deepEqual(readSiyuanCodeBlockConfig({
        codeLigatures: false,
        codeLineWrap: true,
        codeSyntaxHighlightLineNum: false,
    }), {
        ligatures: false,
        lineWrap: true,
        showLineNumbers: false,
    });
});

test("maps enabled native line numbers without changing wrap or ligatures", () => {
    assert.deepEqual(readSiyuanCodeBlockConfig({
        codeLigatures: true,
        codeLineWrap: false,
        codeSyntaxHighlightLineNum: true,
    }), {
        ligatures: true,
        lineWrap: false,
        showLineNumbers: true,
    });
});

test("uses native-safe defaults before the host configuration is available", () => {
    assert.deepEqual(readSiyuanCodeBlockConfig(undefined), {
        ligatures: false,
        lineWrap: true,
        showLineNumbers: false,
    });
});
