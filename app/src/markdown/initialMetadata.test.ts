import assert = require("node:assert/strict");
import test from "node:test";
import {shouldInitializeMarkdownTitle} from "./initialMetadata";

test("opening an external document never initializes Front Matter", () => {
    assert.equal(shouldInitializeMarkdownTitle("external", {status: "none"}, "note"), false);
    assert.equal(shouldInitializeMarkdownTitle("external", {status: "valid", title: "Other"}, "note"), false);
});

test("workspace documents keep the existing title initialization behavior", () => {
    assert.equal(shouldInitializeMarkdownTitle("workspace", {status: "none"}, "note"), true);
    assert.equal(shouldInitializeMarkdownTitle("workspace", {status: "valid", title: "Other"}, "note"), true);
    assert.equal(shouldInitializeMarkdownTitle("workspace", {status: "valid", title: "note"}, "note"), false);
    assert.equal(shouldInitializeMarkdownTitle("workspace", {status: "malformed"}, "note"), false);
});
