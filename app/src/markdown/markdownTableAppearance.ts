import type {
    MarkdownTableAppearancePluginOptions,
    MarkdownTableAppearanceSnapshot,
    MarkdownTableWidthMode,
    PersistedMarkdownTableAppearance,
} from "./markra-core/codemirror";

interface MarkdownTableAppearanceDocumentResponse {
    revision?: number;
    tables?: Record<string, PersistedMarkdownTableAppearance>;
}

interface MarkdownTableAppearanceEvent {
    documentKey?: string;
    origin?: string;
    record?: PersistedMarkdownTableAppearance;
}

interface MarkdownTableAppearanceControllerOptions {
    documentKey: string;
    legacyDocumentKey?: string;
    setExternalAppearanceRetention?: (retained: boolean) => Promise<void> | void;
    request?: MarkdownTableAppearanceRequest;
}

interface MarkdownTableAppearanceResponse {
    code: number;
    data?: any;
}

type MarkdownTableAppearanceRequest = (
    url: string,
    data?: Record<string, unknown>,
) => Promise<MarkdownTableAppearanceResponse>;

type AppearanceListener = (records: readonly PersistedMarkdownTableAppearance[]) => void;

const controllers = new Set<MarkdownTableAppearanceController>();

const requestMarkdownTableAppearance: MarkdownTableAppearanceRequest = async (url, data) => {
    const response = await fetch(url, {
        method: "POST",
        body: JSON.stringify(data),
        headers: {"Content-Type": "application/json"},
    });
    return response.json() as Promise<MarkdownTableAppearanceResponse>;
};

const validWidthMode = (value: unknown): value is MarkdownTableWidthMode => value === "auto" || value === "even";

const normalizedRecord = (value: PersistedMarkdownTableAppearance) => {
    if (!value || typeof value.tableId !== "string" || !validWidthMode(value.attributes?.widthMode)) return null;
    return value;
};

const patchForRecord = (record: MarkdownTableAppearanceSnapshot, includeWidthMode: boolean) => ({
    contentFingerprint: record.contentFingerprint,
    contextFingerprint: record.contextFingerprint,
    columnCount: record.structure.columnCount,
    headerFingerprint: record.structure.headerFingerprint,
    ordinalHint: record.ordinalHint,
    lastMatchedAt: Date.now(),
    ...(includeWidthMode ? {widthMode: record.attributes.widthMode} : {}),
    deleted: includeWidthMode && record.attributes.widthMode === "auto",
});

export class MarkdownTableAppearanceController {
    private documentKey: string;
    private readonly legacyDocumentKey?: string;
    private readonly origin = globalThis.crypto?.randomUUID?.() ?? `appearance-${Date.now()}-${Math.random()}`;
    private readonly records = new Map<string, PersistedMarkdownTableAppearance>();
    private readonly listeners = new Set<AppearanceListener>();
    private readonly patchTimers = new Map<string, number>();
    private readonly pending = new Set<Promise<unknown>>();
    private snapshotTimer?: number;
    private latestSnapshot: readonly MarkdownTableAppearanceSnapshot[] = [];
    private legacyWidthMode?: MarkdownTableWidthMode;
    private legacyMigrated = false;
    private externalAppearanceRetained?: boolean;
    private loaded = false;
    private loading?: Promise<void>;

    constructor(private readonly options: MarkdownTableAppearanceControllerOptions) {
        this.documentKey = options.documentKey;
        this.legacyDocumentKey = options.legacyDocumentKey;
        controllers.add(this);
    }

    async load() {
        if (this.loaded) return;
        if (!this.loading) {
            this.loading = this.loadRecords().finally(() => {
                this.loaded = true;
                this.loading = undefined;
            });
        }
        await this.loading;
    }

    pluginOptions(): MarkdownTableAppearancePluginOptions {
        return {
            defaultWidthMode: this.legacyWidthMode ?? "auto",
            getRecords: () => [...this.records.values()],
            onChange: (record) => this.handleAppearanceChange(record),
            onSnapshot: (records) => this.handleSnapshot(records),
        };
    }

    subscribe(listener: AppearanceListener) {
        this.listeners.add(listener);
        return () => this.listeners.delete(listener);
    }

    async migrate(nextDocumentKey: string) {
        if (nextDocumentKey === this.documentKey) return;
        const previousDocumentKey = this.documentKey;
        this.documentKey = nextDocumentKey;
        const promise = this.request("/api/storage/migrateMarkdownTableAppearance", {
            fromKey: previousDocumentKey,
            toKey: nextDocumentKey,
        });
        this.track(promise);
        await promise.catch(() => undefined);
    }

    applyRemote(event: MarkdownTableAppearanceEvent) {
        if (event.documentKey !== this.documentKey || event.origin === this.origin || !event.record) return;
        const record = normalizedRecord(event.record);
        if (!record) return;
        this.records.set(record.tableId, record);
        void this.syncExternalAppearanceRetention();
        this.listeners.forEach((listener) => listener([record]));
    }

    async flush() {
        window.clearTimeout(this.snapshotTimer);
        this.snapshotTimer = undefined;
        for (const [tableId, timer] of this.patchTimers) {
            window.clearTimeout(timer);
            this.patchTimers.delete(tableId);
            const record = this.latestSnapshot.find((item) => item.tableId === tableId);
            if (record) this.sendPatch(record, true);
        }
        if (this.latestSnapshot.length > 0) this.persistSnapshot();
        await Promise.allSettled([...this.pending]);
    }

    destroy() {
        window.clearTimeout(this.snapshotTimer);
        this.patchTimers.forEach((timer) => window.clearTimeout(timer));
        this.patchTimers.clear();
        this.listeners.clear();
        controllers.delete(this);
    }

    private handleAppearanceChange(record: MarkdownTableAppearanceSnapshot) {
        this.latestSnapshot = this.latestSnapshot.map((item) => item.tableId === record.tableId ? record : item);
        if (!this.latestSnapshot.some((item) => item.tableId === record.tableId)) {
            this.latestSnapshot = [...this.latestSnapshot, record];
        }
        this.records.set(record.tableId, record);
        if (record.attributes.widthMode !== "auto") void this.setExternalAppearanceRetention(true);
        window.clearTimeout(this.patchTimers.get(record.tableId));
        this.patchTimers.set(record.tableId, window.setTimeout(() => {
            this.patchTimers.delete(record.tableId);
            this.sendPatch(record, true);
        }, 300));
    }

    private handleSnapshot(records: readonly MarkdownTableAppearanceSnapshot[]) {
        this.latestSnapshot = records;
        window.clearTimeout(this.snapshotTimer);
        this.snapshotTimer = window.setTimeout(() => {
            this.snapshotTimer = undefined;
            this.persistSnapshot();
        }, 1000);
    }

    private persistSnapshot() {
        const active = new Set(this.latestSnapshot.map((record) => record.tableId));
        this.latestSnapshot.forEach((record) => {
            if (this.legacyWidthMode) {
                this.sendPatch(record, true);
            } else if (record.attributes.widthMode !== "auto" && this.records.has(record.tableId)) {
                this.sendPatch(record, false);
            }
        });
        this.records.forEach((record) => {
            if (active.has(record.tableId) || record.deletedAt) return;
            this.sendRawPatch(record.tableId, {deleted: true});
        });
        if (this.legacyWidthMode && !this.legacyMigrated && this.latestSnapshot.length > 0) {
            this.legacyMigrated = true;
            const migration = Promise.all(this.latestSnapshot.map((record) => this.sendPatch(record, true))).then(() => {
                this.removeLegacyWidthMode();
                this.legacyWidthMode = undefined;
            });
            this.track(migration);
        }
    }

    private sendPatch(record: MarkdownTableAppearanceSnapshot, includeWidthMode: boolean) {
        return this.sendRawPatch(record.tableId, patchForRecord(record, includeWidthMode));
    }

    private sendRawPatch(tableID: string, patch: Record<string, unknown>) {
        const documentKey = this.documentKey;
        const promise = this.request("/api/storage/patchMarkdownTableAppearance", {
            documentKey,
            origin: this.origin,
            patch,
            tableID,
        }).then((response) => {
            if (response.code !== 0 || documentKey !== this.documentKey) return;
            const record = normalizedRecord(response.data?.record as PersistedMarkdownTableAppearance);
            if (record) {
                this.records.set(record.tableId, record);
                void this.syncExternalAppearanceRetention();
            }
        });
        this.track(promise);
        return promise;
    }

    private track<T>(promise: Promise<T>) {
        this.pending.add(promise);
        void promise.catch(() => undefined).finally(() => this.pending.delete(promise));
    }

    private get request() {
        return this.options.request ?? requestMarkdownTableAppearance;
    }

    private async loadRecords() {
        try {
            const response = await this.request("/api/storage/getMarkdownTableAppearance", {documentKey: this.documentKey});
            if (response.code === 0) {
                const data = response.data as MarkdownTableAppearanceDocumentResponse;
                Object.values(data?.tables || {}).forEach((value) => {
                    const record = normalizedRecord(value);
                    if (record) this.records.set(record.tableId, record);
                });
            }
        } catch {
            // 外观持久层不可用时继续打开文档，表格使用默认显示状态。
        }
        void this.syncExternalAppearanceRetention();
        if (this.records.size === 0) this.legacyWidthMode = this.readLegacyWidthMode();
    }

    private syncExternalAppearanceRetention() {
        const retained = [...this.records.values()].some((record) =>
            !record.deletedAt && record.attributes.widthMode !== "auto");
        return this.setExternalAppearanceRetention(retained);
    }

    private async setExternalAppearanceRetention(retained: boolean) {
        if (this.externalAppearanceRetained === retained || !this.documentKey.startsWith("external:")) return;
        const previous = this.externalAppearanceRetained;
        this.externalAppearanceRetained = retained;
        try {
            await this.options.setExternalAppearanceRetention?.(retained);
        } catch {
            this.externalAppearanceRetained = previous;
        }
    }

    private legacyStorageKey() {
        const key = this.legacyDocumentKey?.trim() || "untitled";
        return `markra:table-width-mode:${encodeURIComponent(key)}`;
    }

    private readLegacyWidthMode() {
        try {
            const value = window.localStorage.getItem(this.legacyStorageKey());
            return validWidthMode(value) ? value : undefined;
        } catch {
            return undefined;
        }
    }

    private removeLegacyWidthMode() {
        try {
            window.localStorage.removeItem(this.legacyStorageKey());
        } catch {
            // 嵌入环境可能禁止访问浏览器存储，迁移失败不影响新的持久化路径。
        }
    }
}

export const applyMarkdownTableAppearanceEvent = (event: MarkdownTableAppearanceEvent) => {
    controllers.forEach((controller) => controller.applyRemote(event));
};
