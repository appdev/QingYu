export interface AnnotationAnchor {
    from: number;
    to: number;
    quote: string;
    prefix: string;
    suffix: string;
    status: "attached" | "orphaned";
}

export interface MarkdownAnnotation extends AnnotationAnchor {
    id: string;
    note: string;
    createdAt: number;
    updatedAt: number;
}

export interface MarkdownAnnotations {
    etag?: string;
    schemaVersion: 1;
    documentId: string;
    revision: number;
    contentHash: string;
    records: MarkdownAnnotation[];
}

export const normalizeAnnotationText = (text: string) => text.replace(/^\uFEFF/u, "").replace(/\r\n?/gu, "\n");

export const emptyAnnotations = (): MarkdownAnnotations => ({
    schemaVersion: 1,
    documentId: Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join(""),
    revision: 0, contentHash: "", records: [],
});

export const parseAnnotations = (value: unknown): MarkdownAnnotations => {
    const data = value as MarkdownAnnotations;
    if (!data || data.schemaVersion !== 1 || typeof data.documentId !== "string" || !/^[\w-]{1,128}$/u.test(data.documentId) ||
        !Number.isSafeInteger(data.revision) || data.revision < 0 || data.revision >= Number.MAX_SAFE_INTEGER ||
        typeof data.contentHash !== "string" || !/^(?:[a-f\d]{64})?$/u.test(data.contentHash) ||
        !Array.isArray(data.records) || data.records.length > 10000) throw new Error("INVALID_ANNOTATIONS");
    const ids = new Set<string>();
    for (const record of data.records) {
        if (!record || typeof record.id !== "string" || !/^[\w-]{1,128}$/u.test(record.id) || ids.has(record.id) ||
            !Number.isSafeInteger(record.from) || !Number.isSafeInteger(record.to) ||
            record.from < 0 || record.to < record.from ||
            !["attached", "orphaned"].includes(record.status) ||
            typeof record.quote !== "string" || !record.quote ||
            typeof record.note !== "string" || record.note.length > 65536 ||
            typeof record.prefix !== "string" || Array.from(record.prefix).length > 64 ||
            typeof record.suffix !== "string" || Array.from(record.suffix).length > 64 ||
            !Number.isSafeInteger(record.createdAt) || record.createdAt < 0 ||
            !Number.isSafeInteger(record.updatedAt) || record.updatedAt < 0) throw new Error("INVALID_ANNOTATIONS");
        ids.add(record.id);
    }
    return {...data, records: data.records.map((record) => ({...record}))};
};
