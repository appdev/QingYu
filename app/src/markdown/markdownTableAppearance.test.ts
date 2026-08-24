import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "./markraTestDom";
import type {MarkdownTableAppearanceSnapshot, PersistedMarkdownTableAppearance} from "./markra-core/codemirror";

Object.assign(globalThis, {NODE_ENV: "test", SIYUAN_VERSION: "test"});

const createController = async (options: ConstructorParameters<
    typeof import("./markdownTableAppearance").MarkdownTableAppearanceController
>[0]) => {
    const {MarkdownTableAppearanceController} = await import("./markdownTableAppearance");
    return new MarkdownTableAppearanceController(options);
};

let cleanup: () => void;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => cleanup());

const snapshot = (tableId = "table-1"): MarkdownTableAppearanceSnapshot => ({
    attributes: {widthMode: "even"},
    contentFingerprint: "content",
    contextFingerprint: "context",
    from: 0,
    ordinalHint: 0,
    structure: {columnCount: 2, headerFingerprint: "header"},
    tableId,
    to: 20,
});

const createRequest = () => {
    const documents = new Map<string, Map<string, PersistedMarkdownTableAppearance>>();
    const request = async (url: string, data?: Record<string, any>) => {
        if (url.endsWith("getMarkdownTableAppearance")) {
            return {code: 0, data: {tables: Object.fromEntries(documents.get(data.documentKey) || [])}};
        }
        if (url.endsWith("patchMarkdownTableAppearance")) {
            const tables = documents.get(data.documentKey) || new Map<string, PersistedMarkdownTableAppearance>();
            documents.set(data.documentKey, tables);
            const previous = tables.get(data.tableID);
            const record = {
                ...previous,
                attributes: {widthMode: data.patch.widthMode ?? previous?.attributes.widthMode ?? "auto"},
                contentFingerprint: data.patch.contentFingerprint ?? previous?.contentFingerprint ?? "",
                contextFingerprint: data.patch.contextFingerprint ?? previous?.contextFingerprint ?? "",
                deletedAt: data.patch.deleted ? Date.now() : undefined,
                ordinalHint: data.patch.ordinalHint ?? previous?.ordinalHint ?? 0,
                structure: {
                    columnCount: data.patch.columnCount ?? previous?.structure.columnCount ?? 0,
                    headerFingerprint: data.patch.headerFingerprint ?? previous?.structure.headerFingerprint ?? "",
                },
                tableId: data.tableID,
            } satisfies PersistedMarkdownTableAppearance;
            tables.set(data.tableID, record);
            return {code: 0, data: {record}};
        }
        if (url.endsWith("migrateMarkdownTableAppearance")) {
            const source = documents.get(data.fromKey);
            if (source) documents.set(data.toKey, source);
            documents.delete(data.fromKey);
            return {code: 0, data: null};
        }
        throw new Error(`unexpected request: ${url}`);
    };
    return {documents, request};
};

test("persists table appearance across controller recreation", async () => {
    const server = createRequest();
    const first = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await first.load();
    first.pluginOptions().onChange?.(snapshot());
    await first.flush();
    first.destroy();

    const second = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await second.load();
    assert.equal(second.pluginOptions().getRecords?.()[0].attributes.widthMode, "even");
    second.destroy();
});

test("migrates the legacy document-wide width mode once", async () => {
    const server = createRequest();
    const legacyKey = `markra:table-width-mode:${encodeURIComponent("/note.md")}`;
    const storage = new Map<string, string>();
    Object.defineProperty(window, "localStorage", {configurable: true, value: {
        getItem: (key: string) => storage.get(key) ?? null,
        removeItem: (key: string) => storage.delete(key),
        setItem: (key: string, value: string) => storage.set(key, value),
    }});
    window.localStorage.setItem(legacyKey, "even");
    const controller = await createController({
        documentKey: "workspace:box:/note.md",
        legacyDocumentKey: "/note.md",
        request: server.request,
    });
    await controller.load();
    const options = controller.pluginOptions();

    assert.equal(options.defaultWidthMode, "even");
    options.onSnapshot?.([snapshot()]);
    await controller.flush();
    assert.equal(window.localStorage.getItem(legacyKey), null);
    assert.equal(server.documents.get("workspace:box:/note.md")?.get("table-1")?.attributes.widthMode, "even");
    controller.destroy();
});

test("appearance storage failure never blocks document loading", async () => {
    const controller = await createController({
        documentKey: "external:capability",
        request: async () => {
            throw new Error("offline");
        },
    });

    await assert.doesNotReject(() => controller.load());
    assert.deepEqual(controller.pluginOptions().getRecords?.(), []);
    controller.destroy();
});

test("releases an external capability after its last custom appearance returns to default", async () => {
    const server = createRequest();
    server.documents.set("external:capability", new Map([["table-1", snapshot()]]));
    const retention: boolean[] = [];
    const controller = await createController({
        documentKey: "external:capability",
        request: server.request,
        setExternalAppearanceRetention: (retained) => {
            retention.push(retained);
        },
    });
    await controller.load();
    controller.pluginOptions().onChange?.({
        ...snapshot(),
        attributes: {widthMode: "auto"},
    });
    await controller.flush();

    assert.deepEqual(retention, [true, false]);
    controller.destroy();
});
