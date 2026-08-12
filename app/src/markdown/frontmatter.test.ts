import assert = require("node:assert/strict");
import test from "node:test";
import {
    readMarkdownFrontmatter,
    upsertMarkdownFrontmatterMetadata,
    upsertMarkdownFrontmatterTitle,
} from "./markra-core/markdown/frontmatter";

test("reads portable Markdown metadata from YAML", () => {
    const result = readMarkdownFrontmatter("---\r\ncustom: keep\r\ntitle: Old\r\ntags: [one, two]\r\nicon: \"🌱\"\r\ncover: assets/cover.png\r\n---\r\nBody");
    assert.equal(result.status, "valid");
    if (result.status !== "valid") return;
    assert.equal(result.title, "Old");
    assert.deepEqual(result.tags, ["one", "two"]);
    assert.equal(result.icon, "🌱");
    assert.equal(result.cover, "assets/cover.png");
});

test("updates YAML metadata without replacing unknown fields or newline style", () => {
    const source = "---\r\ncustom: keep\r\ntitle: Old\r\n---\r\n\r\nBody";
    const result = upsertMarkdownFrontmatterMetadata(source, {
        title: "New",
        tags: ["one", "two"],
        icon: "🌱",
        cover: "assets/cover.png",
    });
    assert.equal(result.ok, true);
    if (!result.ok) return;
    assert.match(result.source, /custom: keep/u);
    assert.equal(result.source.replace(/\r\n/gu, "").includes("\n"), false);
    const metadata = readMarkdownFrontmatter(result.source);
    assert.equal(metadata.status, "valid");
    if (metadata.status !== "valid") return;
    assert.deepEqual(metadata.tags, ["one", "two"]);
    assert.equal(metadata.icon, "🌱");
    assert.equal(metadata.cover, "assets/cover.png");
});

test("updates TOML and JSON metadata while retaining unrelated fields", () => {
    const toml = upsertMarkdownFrontmatterMetadata("+++\ntitle = 'Old'\ncustom = 1\n+++\nBody", {
        tags: ["one"],
        icon: "🌱",
    });
    assert.equal(toml.ok, true);
    if (toml.ok) {
        assert.match(toml.source, /custom = 1/u);
        const metadata = readMarkdownFrontmatter(toml.source);
        assert.equal(metadata.status, "valid");
        if (metadata.status === "valid") assert.deepEqual(metadata.tags, ["one"]);
    }

    const json = upsertMarkdownFrontmatterMetadata('{\n  "title": "Old",\n  "custom": true\n}\nBody', {
        cover: "https://example.com/cover.png",
    });
    assert.equal(json.ok, true);
    if (json.ok) {
        assert.match(json.source, /"custom": true/u);
        const metadata = readMarkdownFrontmatter(json.source);
        assert.equal(metadata.status, "valid");
        if (metadata.status === "valid") assert.equal(metadata.cover, "https://example.com/cover.png");
    }
});

test("creates YAML metadata and refuses malformed frontmatter", () => {
    const created = upsertMarkdownFrontmatterMetadata("\ufeffBody", {tags: ["one"], icon: "🌱"});
    assert.equal(created.ok, true);
    if (created.ok) assert.match(created.source, /^\ufeff---\ntags:/u);

    assert.deepEqual(upsertMarkdownFrontmatterTitle("---\ntitle: broken", "New"), {
        ok: false,
        reason: "malformed",
    });
});
