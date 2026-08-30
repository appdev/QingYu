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

test("keeps the table identity persisted when its width returns to auto", async () => {
    const server = createRequest();
    const first = await createController({
        documentKey: "workspace:box:/auto.md",
        request: server.request,
    });
    await first.load();
    first.pluginOptions().onChange?.({...snapshot("stable-auto"), attributes: {widthMode: "auto"}});
    await first.flush();
    first.destroy();

    const second = await createController({
        documentKey: "workspace:box:/auto.md",
        request: server.request,
    });
    await second.load();
    const restored = second.pluginOptions().getRecords?.()[0];
    assert.equal(restored?.tableId, "stable-auto");
    assert.equal(restored?.attributes.widthMode, "auto");
    assert.equal(restored?.deletedAt, undefined);
    second.destroy();
});

test("ignores an older remote record after a newer width state is cached", async () => {
    const server = createRequest();
    server.documents.set("workspace:box:/note.md", new Map([[
        "table-1",
        {...snapshot(), updatedAt: 20, version: 20},
    ]]));
    const controller = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await controller.load();

    controller.applyRemote({
        documentKey: "workspace:box:/note.md",
        origin: "another-editor",
        record: {
            ...snapshot(),
            attributes: {widthMode: "auto"},
            deletedAt: 10,
            updatedAt: 10,
            version: 10,
        },
    });

    const current = controller.pluginOptions().getRecords?.()[0];
    assert.equal(current?.attributes.widthMode, "even");
    assert.equal(current?.deletedAt, undefined);
    assert.equal(current?.version, 20);
    controller.destroy();
});

test("ignores older patch responses that finish after the latest table update", async () => {
    const pending: Array<{
        data: Record<string, any>;
        resolve: (response: {code: number; data: {record: PersistedMarkdownTableAppearance}}) => void;
    }> = [];
    const request = async (url: string, data?: Record<string, any>) => {
        if (url.endsWith("getMarkdownTableAppearance")) {
            return {code: 0, data: {tables: {"table-1": {...snapshot(), version: 1}}}};
        }
        if (url.endsWith("patchMarkdownTableAppearance")) {
            return new Promise<{code: number; data: {record: PersistedMarkdownTableAppearance}}>((resolve) => {
                pending.push({data, resolve});
            });
        }
        throw new Error(`unexpected request: ${url}`);
    };
    const controller = await createController({documentKey: "workspace:box:/note.md", request});
    await controller.load();
    const options = controller.pluginOptions();

    options.onDelete?.(["table-1"]);
    options.onChange?.({...snapshot(), version: 1});
    const flushed = controller.flush();
    assert.equal(pending.length, 3);

    [...pending].reverse().forEach((item, reverseIndex) => {
        const version = pending.length + 1 - reverseIndex;
        const deleted = item.data.patch.deleted === true;
        item.resolve({code: 0, data: {record: {
            ...snapshot(),
            attributes: {widthMode: deleted ? "auto" : "even"},
            ...(deleted ? {deletedAt: version} : {}),
            updatedAt: version,
            version,
        }}});
    });
    await flushed;

    const current = options.getRecords?.()[0];
    assert.equal(current?.attributes.widthMode, "even");
    assert.equal(current?.deletedAt, undefined);
    assert.equal(current?.version, 4);
    controller.destroy();
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

test("does not delete persisted tables that are absent from a partial parser snapshot", async () => {
    const server = createRequest();
    server.documents.set("workspace:box:/note.md", new Map([
        ["visible", snapshot("visible")],
        ["offscreen", snapshot("offscreen")],
    ]));
    const controller = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await controller.load();

    controller.pluginOptions().onSnapshot?.([snapshot("visible")]);
    await controller.flush();

    assert.equal(server.documents.get("workspace:box:/note.md")?.get("offscreen")?.deletedAt, undefined);
    controller.destroy();
});

test("deletes appearance only when the editor reports a concrete table identity", async () => {
    const server = createRequest();
    server.documents.set("workspace:box:/note.md", new Map([["table-1", snapshot()]]));
    const controller = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await controller.load();

    controller.pluginOptions().onDelete?.(["table-1"]);
    await controller.flush();

    assert.ok(server.documents.get("workspace:box:/note.md")?.get("table-1")?.deletedAt);
    controller.destroy();
});

test("flushes the pending width change instead of a later partial snapshot fallback", async () => {
    const server = createRequest();
    const controller = await createController({
        documentKey: "workspace:box:/note.md",
        request: server.request,
    });
    await controller.load();

    controller.pluginOptions().onChange?.(snapshot());
    controller.pluginOptions().onSnapshot?.([{...snapshot(), attributes: {widthMode: "auto"}}]);
    await controller.flush();

    const persisted = server.documents.get("workspace:box:/note.md")?.get("table-1");
    assert.equal(persisted?.attributes.widthMode, "even");
    assert.equal(persisted?.deletedAt, undefined);
    controller.destroy();
});
