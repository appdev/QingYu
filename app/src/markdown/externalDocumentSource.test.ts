import assert = require("node:assert/strict");
import test from "node:test";
import {createExternalMarkdownDocumentSource} from "./externalDocumentSource";

test("external document operations send only the capability identifier", async () => {
    const calls: Array<{channel: string, payload: Record<string, unknown>}> = [];
    const source = createExternalMarkdownDocumentSource({
        capabilityId: "cap-1",
        readOnly: false,
        invoke: async (channel, payload) => {
            calls.push({channel, payload});
            return {
                status: "ok",
                document: {
                    name: "note.md",
                    displayPath: "/outside/note.md",
                    content: "# Note\n",
                    revision: "r1",
                    mtime: 1,
                    utf8Bom: false,
                    lineEnding: "\n",
                },
            };
        },
    });

    const loaded = await source.load();
    await source.save({content: "changed\n", revision: loaded.revision});

    assert.equal(loaded.name, "note.md");
    assert.deepEqual(calls, [
        {channel: "siyuan-external-markdown", payload: {action: "read", capabilityId: "cap-1"}},
        {
            channel: "siyuan-external-markdown",
            payload: {action: "save", capabilityId: "cap-1", request: {content: "changed\n", revision: "r1"}},
        },
    ]);
});

test("a read-only external source rejects mutations before IPC", async () => {
    let invoked = false;
    const source = createExternalMarkdownDocumentSource({
        capabilityId: "cap-2",
        readOnly: true,
        invoke: async () => {
            invoked = true;
            return undefined;
        },
    });

    const result = await source.save({content: "changed", revision: "r1"});

    assert.deepEqual(result, {status: "error", code: "READ_ONLY"});
    assert.equal(invoked, false);
});

test("external image resolution keeps safe remote images and capabilities local relative images", async () => {
    const source = createExternalMarkdownDocumentSource({
        capabilityId: "cap-3",
        readOnly: false,
        invoke: async () => ({
            status: "ok",
            document: {
                name: "note.md", displayPath: "/note.md", content: "", revision: "r1", mtime: 1,
                utf8Bom: false, lineEnding: "\n", resourceToken: "token-1",
            },
        }),
    });
    await source.load();

    assert.equal(source.resolveImageSource("https://example.com/image.png"), "https://example.com/image.png");
    assert.equal(source.resolveImageSource("images/photo 1.png"),
        "qingyu-external-resource://resource/cap-3/token-1/images/photo%201.png");
    assert.equal(source.resolveImageSource("javascript:alert(1)"), null);
});
