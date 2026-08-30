import {syntaxTree} from "@codemirror/language";
import {
    MapMode,
    StateEffect,
    StateField,
    type EditorState,
    type Extension,
    type Transaction,
} from "@codemirror/state";
import {EditorView, ViewPlugin} from "@codemirror/view";

export type MarkdownTableWidthMode = "auto" | "even";

export interface MarkdownTableAppearanceAttributes {
    readonly widthMode: MarkdownTableWidthMode;
}

export interface MarkdownTableAppearanceStructure {
    readonly columnCount: number;
    readonly headerFingerprint: string;
}

export interface PersistedMarkdownTableAppearance {
    readonly tableId: string;
    readonly contentFingerprint: string;
    readonly contextFingerprint: string;
    readonly structure: MarkdownTableAppearanceStructure;
    readonly ordinalHint: number;
    readonly attributes: MarkdownTableAppearanceAttributes;
    readonly version?: number;
    readonly updatedAt?: number;
    readonly lastMatchedAt?: number;
    readonly deletedAt?: number;
}

export interface MarkdownTableAppearanceSnapshot extends PersistedMarkdownTableAppearance {
    readonly from: number;
    readonly to: number;
}

export interface MarkdownTableAppearancePluginOptions {
    readonly defaultWidthMode?: MarkdownTableWidthMode;
    readonly getRecords?: () => readonly PersistedMarkdownTableAppearance[];
    readonly onChange?: (record: MarkdownTableAppearanceSnapshot) => void;
    readonly onDelete?: (tableIds: readonly string[]) => void;
    readonly onSnapshot?: (records: readonly MarkdownTableAppearanceSnapshot[]) => void;
}

interface TableDescriptor {
    readonly contentFingerprint: string;
    readonly contextFingerprint: string;
    readonly from: number;
    readonly ordinalHint: number;
    readonly structure: MarkdownTableAppearanceStructure;
    readonly to: number;
}

interface MarkdownTableAppearanceState {
    readonly entries: readonly MarkdownTableAppearanceSnapshot[];
    readonly moveTokens: readonly MarkdownTableAppearanceMoveToken[];
}

interface MarkdownTableAppearanceMoveToken {
    readonly createdAt: number;
    readonly entry: MarkdownTableAppearanceSnapshot;
}

interface SetWidthModeEffect {
    readonly from: number;
    readonly mode: MarkdownTableWidthMode;
    readonly tableId: string;
}

interface EnsureTableAppearanceEffect {
    readonly from: number;
    readonly tableId: string;
}

let fallbackTableID = 0;

const newTableID = () => {
    const randomUUID = globalThis.crypto?.randomUUID?.bind(globalThis.crypto);
    if (randomUUID) return randomUUID();
    fallbackTableID += 1;
    return `markra-table-${Date.now().toString(36)}-${fallbackTableID.toString(36)}`;
};

const stableHash = (value: string) => {
    let first = 0x811c9dc5;
    let second = 0x9e3779b9;
    for (let index = 0; index < value.length; index++) {
        const code = value.charCodeAt(index);
        first = Math.imul(first ^ code, 0x01000193);
        second = Math.imul(second ^ code, 0x85ebca6b);
    }
    return `${(first >>> 0).toString(16).padStart(8, "0")}${(second >>> 0).toString(16).padStart(8, "0")}`;
};

const normalizeTableLine = (line: string) => line.trim().replace(/\s+/gu, " ");

const tableColumnCount = (header: string) => {
    const value = header.trim().replace(/^\|/u, "").replace(/\|$/u, "");
    if (!value) return 0;
    let count = 1;
    let escaped = false;
    for (const character of value) {
        if (character === "|" && !escaped) count += 1;
        escaped = character === "\\" ? !escaped : false;
    }
    return count;
};

const nearestContextLine = (state: EditorState, lineNumber: number, direction: -1 | 1) => {
    for (let current = lineNumber + direction; current > 0 && current <= state.doc.lines; current += direction) {
        const text = state.doc.line(current).text.trim();
        if (text) return text;
    }
    return "";
};

const nearestHeading = (state: EditorState, lineNumber: number) => {
    for (let current = lineNumber - 1; current > 0; current--) {
        const text = state.doc.line(current).text.trim();
        if (/^#{1,6}\s+/u.test(text)) return text.replace(/\s+/gu, " ");
    }
    return "";
};

export const readMarkdownTableDescriptors = (state: EditorState): readonly TableDescriptor[] => {
    const descriptors: TableDescriptor[] = [];
    syntaxTree(state).iterate({
        enter(node) {
            if (node.name !== "Table") return;
            const firstLine = state.doc.lineAt(node.from);
            const lastLine = state.doc.lineAt(node.to);
            const source = state.sliceDoc(firstLine.from, lastLine.to);
            const lines = source.split("\n").map(normalizeTableLine);
            const header = lines[0] || "";
            const context = [
                nearestHeading(state, firstLine.number),
                nearestContextLine(state, firstLine.number, -1),
                nearestContextLine(state, lastLine.number, 1),
            ].join("\n");
            descriptors.push({
                contentFingerprint: stableHash(lines.join("\n")),
                contextFingerprint: stableHash(context),
                from: node.from,
                ordinalHint: descriptors.length,
                structure: {
                    columnCount: tableColumnCount(header),
                    headerFingerprint: stableHash(header),
                },
                to: node.to,
            });
        },
    });
    return descriptors;
};

const appearanceScore = (descriptor: TableDescriptor, record: PersistedMarkdownTableAppearance) => {
    let score = 0;
    if (descriptor.contentFingerprint === record.contentFingerprint) score += 1;
    if (descriptor.contextFingerprint === record.contextFingerprint) score += 0.45;
    if (descriptor.structure.headerFingerprint === record.structure.headerFingerprint) score += 0.2;
    if (descriptor.structure.columnCount === record.structure.columnCount) score += 0.1;
    if (descriptor.ordinalHint === record.ordinalHint) score += 0.05;
    return score;
};

export const matchMarkdownTableAppearances = (
    descriptors: readonly TableDescriptor[],
    records: readonly PersistedMarkdownTableAppearance[],
) => {
    const matched = new Map<number, PersistedMarkdownTableAppearance>();
    const used = new Set<string>();
    const matchPool = (
        available: readonly PersistedMarkdownTableAppearance[],
        minimumScore: number,
        minimumMargin: number,
    ) => {
        descriptors.forEach((descriptor, descriptorIndex) => {
            if (matched.has(descriptorIndex)) return;
            const exact = available.filter((record) => !used.has(record.tableId) &&
                record.contentFingerprint === descriptor.contentFingerprint &&
                record.contextFingerprint === descriptor.contextFingerprint);
            if (exact.length !== 1) return;
            matched.set(descriptorIndex, exact[0]);
            used.add(exact[0].tableId);
        });

        descriptors.forEach((descriptor, descriptorIndex) => {
            if (matched.has(descriptorIndex)) return;
            const candidates = available
                .filter((record) => !used.has(record.tableId))
                .map((record) => ({record, score: appearanceScore(descriptor, record)}))
                .sort((left, right) => right.score - left.score);
            const best = candidates[0];
            const next = candidates[1];
            if (!best || best.score < minimumScore || next && best.score - next.score < minimumMargin) return;
            matched.set(descriptorIndex, best.record);
            used.add(best.record.tableId);
        });
    };

    matchPool(records.filter((record) => !record.deletedAt), 0.65, 0.15);
    matchPool(records.filter((record) => record.deletedAt && record.attributes.widthMode !== "auto"), 0.8, 0.2);
    return matched;
};

const snapshotFrom = (
    descriptor: TableDescriptor,
    tableId: string,
    widthMode: MarkdownTableWidthMode,
    version = 0,
): MarkdownTableAppearanceSnapshot => ({
    ...descriptor,
    attributes: {widthMode},
    tableId,
    version,
});

export const resolveMarkdownTableAppearance = (
    state: EditorState,
    from: number,
    records: readonly PersistedMarkdownTableAppearance[],
    defaultWidthMode: MarkdownTableWidthMode,
) => {
    const descriptor = readMarkdownTableDescriptors(state).find((item) =>
        item.from === from || item.from < from && item.to >= from);
    if (!descriptor) return undefined;
    const record = matchMarkdownTableAppearances([descriptor], records).get(0);
    return snapshotFrom(
        descriptor,
        record?.tableId ?? `table-${descriptor.from}`,
        record?.attributes.widthMode ?? defaultWidthMode,
        record?.version,
    );
};

const mappedEntry = (transaction: Transaction, entry: MarkdownTableAppearanceSnapshot) => {
    const first = transaction.changes.mapPos(entry.from, 1, MapMode.Simple);
    const last = transaction.changes.mapPos(entry.to, -1, MapMode.Simple);
    if (first !== null && last !== null && first !== last) {
        return {...entry, from: Math.min(first, last), to: Math.max(first, last)};
    }
    let replacement: {from: number; to: number} | undefined;
    transaction.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
        if (replacement || fromB === toB || fromA > entry.from || toA < entry.to) return;
        replacement = {from: fromB, to: toB};
    });
    return replacement ? {...entry, ...replacement} : null;
};

const entryForDescriptor = (
    descriptor: TableDescriptor,
    entries: readonly MarkdownTableAppearanceSnapshot[],
    used: Set<string>,
) => {
    const candidates = entries.filter((entry) => !used.has(entry.tableId) &&
        (entry.from === descriptor.from || entry.from < descriptor.to && entry.to > descriptor.from));
    return candidates.find((entry) => entry.attributes.widthMode !== "auto") ?? candidates[0];
};

const movedEntryForDescriptor = (
    descriptor: TableDescriptor,
    tokens: readonly MarkdownTableAppearanceMoveToken[],
    used: Set<string>,
) => {
    const candidates = tokens.map((token) => token.entry).filter((entry) => !used.has(entry.tableId) &&
        entry.contentFingerprint === descriptor.contentFingerprint);
    return candidates.length === 1 ? candidates[0] : undefined;
};

const replacementEntries = (
    transaction: Transaction,
    removed: readonly MarkdownTableAppearanceSnapshot[],
    descriptors: readonly TableDescriptor[],
) => {
    const replacements = new Map<number, MarkdownTableAppearanceSnapshot>();
    transaction.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
        if (fromA === toA || fromB === toB) return;
        const previous = removed.filter((entry) => entry.from < toA && entry.to > fromA);
        const current = descriptors
            .map((descriptor, index) => ({descriptor, index}))
            .filter(({descriptor}) => descriptor.from < toB && descriptor.to > fromB);
        if (previous.length === 1 && current.length === 1) {
            replacements.set(current[0].index, previous[0]);
        }
    });
    return replacements;
};

export const deleteMarkdownTableAppearance = StateEffect.define<{readonly tableId: string}>();
export const ensureMarkdownTableAppearance = StateEffect.define<EnsureTableAppearanceEffect>();
export const setMarkdownTableWidthMode = StateEffect.define<SetWidthModeEffect>();
export const restoreMarkdownTableAppearances = StateEffect.define<readonly PersistedMarkdownTableAppearance[]>();

export const createMarkdownTableAppearanceExtension = (
    options: MarkdownTableAppearancePluginOptions = {},
): {extension: Extension; field: StateField<MarkdownTableAppearanceState>} => {
    const defaultWidthMode = options.defaultWidthMode ?? "auto";
    const field = StateField.define<MarkdownTableAppearanceState>({
        create(state) {
            const descriptors = readMarkdownTableDescriptors(state);
            const persisted = options.getRecords?.() ?? [];
            const matches = matchMarkdownTableAppearances(descriptors, persisted);
            return {
                entries: descriptors.map((descriptor, index) => {
                    const record = matches.get(index);
                    return snapshotFrom(
                        descriptor,
                        record?.tableId ?? newTableID(),
                        record?.attributes.widthMode ?? defaultWidthMode,
                        record?.version,
                    );
                }),
                moveTokens: [],
            };
        },
        update(value, transaction) {
            let entries = value.entries;
            let moveTokens = value.moveTokens.filter((token) => Date.now() - token.createdAt <= 5000);
            const registrations = transaction.effects
                .filter((effect) => effect.is(ensureMarkdownTableAppearance))
                .map((effect) => effect.value);
            if (transaction.docChanged) {
                const mapped = entries.map((entry) => mappedEntry(transaction, entry)).filter(
                    (entry): entry is MarkdownTableAppearanceSnapshot => Boolean(entry),
                );
                const mappedIDs = new Set(mapped.map((entry) => entry.tableId));
                const removed = entries.filter((entry) => !mappedIDs.has(entry.tableId));
                if (transaction.isUserEvent("delete.cut") || transaction.isUserEvent("move")) {
                    const createdAt = Date.now();
                    moveTokens = [...moveTokens, ...removed.map((entry) => ({createdAt, entry}))].slice(-32);
                }
                const descriptors = readMarkdownTableDescriptors(transaction.state);
                const replacements = replacementEntries(transaction, removed, descriptors);
                const used = new Set<string>();
                const sources = descriptors.map((descriptor, index) => {
                    const current = entryForDescriptor(descriptor, mapped, used);
                    const moved = current ? undefined : movedEntryForDescriptor(descriptor, moveTokens, used);
                    const replaced = current || moved ? undefined : replacements.get(index);
                    const source = current ?? moved ?? replaced;
                    if (source) used.add(source.tableId);
                    return source;
                });
                const unresolvedIndexes = descriptors
                    .map((_descriptor, index) => index)
                    .filter((index) => !sources[index]);
                const retainedMatches = matchMarkdownTableAppearances(
                    unresolvedIndexes.map((index) => descriptors[index]),
                    mapped.filter((entry) => !used.has(entry.tableId)),
                );
                unresolvedIndexes.forEach((descriptorIndex, unresolvedIndex) => {
                    const retained = retainedMatches.get(unresolvedIndex);
                    if (!retained) return;
                    sources[descriptorIndex] = retained as MarkdownTableAppearanceSnapshot;
                    used.add(retained.tableId);
                });
                const persistedIndexes = unresolvedIndexes.filter((index) => !sources[index]);
                const persistedMatches = matchMarkdownTableAppearances(
                    persistedIndexes.map((index) => descriptors[index]),
                    (options.getRecords?.() ?? []).filter((record) => !used.has(record.tableId)),
                );
                const persistedByDescriptor = new Map<number, PersistedMarkdownTableAppearance>();
                persistedIndexes.forEach((descriptorIndex, unresolvedIndex) => {
                    const persisted = persistedMatches.get(unresolvedIndex);
                    if (persisted) persistedByDescriptor.set(descriptorIndex, persisted);
                });
                const parsed = descriptors.map((descriptor, index) => {
                    const source = sources[index];
                    const persisted = source ? undefined : persistedByDescriptor.get(index);
                    const registration = registrations.find((item) =>
                        item.from === descriptor.from || descriptor.from < item.from && descriptor.to >= item.from);
                    const tableId = source?.tableId ?? persisted?.tableId ?? registration?.tableId ?? newTableID();
                    used.add(tableId);
                    return snapshotFrom(
                        descriptor,
                        tableId,
                        source?.attributes.widthMode ?? persisted?.attributes.widthMode ?? defaultWidthMode,
                        source?.version ?? persisted?.version,
                    );
                });
                entries = [
                    ...parsed,
                    ...mapped.filter((entry) => !used.has(entry.tableId)),
                ].sort((left, right) => left.from - right.from);
                moveTokens = moveTokens.filter((token) => !used.has(token.entry.tableId));
            }
            for (const registration of registrations) {
                const target = entries.find((entry) => entry.from === registration.from) ??
                    entries.find((entry) => entry.tableId === registration.tableId);
                if (target) continue;
                const record = resolveMarkdownTableAppearance(
                    transaction.state,
                    registration.from,
                    options.getRecords?.() ?? [],
                    defaultWidthMode,
                );
                if (record) {
                    entries = [
                        ...entries.filter((entry) => entry.from !== record.from),
                        {...record, tableId: registration.tableId},
                    ].sort((left, right) => left.from - right.from);
                }
            }
            for (const effect of transaction.effects) {
                if (!effect.is(setMarkdownTableWidthMode)) continue;
                const target = entries.find((entry) => entry.from === effect.value.from) ??
                    entries.find((entry) => entry.tableId === effect.value.tableId);
                entries = entries.map((entry) => {
                    if (entry !== target) return entry;
                    return {...entry, attributes: {widthMode: effect.value.mode}};
                });
                if (!target) {
                    const record = resolveMarkdownTableAppearance(
                        transaction.state,
                        effect.value.from,
                        options.getRecords?.() ?? [],
                        defaultWidthMode,
                    );
                    if (record) {
                        entries = [
                            ...entries.filter((entry) => entry.from !== record.from),
                            {
                                ...record,
                                attributes: {widthMode: effect.value.mode},
                                tableId: effect.value.tableId,
                            },
                        ].sort((left, right) => left.from - right.from);
                    }
                }
            }
            for (const effect of transaction.effects) {
                if (!effect.is(restoreMarkdownTableAppearances)) continue;
                const records = effect.value;
                const descriptors = entries.map((entry) => ({
                    contentFingerprint: entry.contentFingerprint,
                    contextFingerprint: entry.contextFingerprint,
                    from: entry.from,
                    ordinalHint: entry.ordinalHint,
                    structure: entry.structure,
                    to: entry.to,
                }));
                const matches = matchMarkdownTableAppearances(descriptors, records);
                entries = entries.map((entry, index) => {
                    const direct = records.find((record) => record.tableId === entry.tableId);
                    const record = direct ?? matches.get(index);
                    return record ? snapshotFrom(
                        descriptors[index],
                        record.tableId,
                        record.attributes.widthMode,
                        record.version,
                    ) : entry;
                });
            }
            return {entries, moveTokens};
        },
    });
    const listener = EditorView.updateListener.of((update) => {
        const state = update.state.field(field, false);
        if (!state) return;
        const deleted = new Set<string>();
        for (const transaction of update.transactions) {
            for (const effect of transaction.effects) {
                if (effect.is(setMarkdownTableWidthMode)) {
                    const record = state.entries.find((entry) => entry.from === effect.value.from) ??
                        state.entries.find((entry) => entry.tableId === effect.value.tableId);
                    if (record) options.onChange?.(record);
                } else if (effect.is(deleteMarkdownTableAppearance)) {
                    deleted.add(effect.value.tableId);
                }
            }
        }
        if (deleted.size > 0) options.onDelete?.([...deleted]);
        if (update.docChanged) options.onSnapshot?.(state.entries);
    });
    const initialSnapshot = ViewPlugin.fromClass(class {
        constructor(view: EditorView) {
            queueMicrotask(() => {
                const state = view.state.field(field, false);
                if (state) options.onSnapshot?.(state.entries);
            });
        }
    });
    return {extension: [field, listener, initialSnapshot], field};
};


export const markdownTableAppearanceAt = (
    state: EditorState,
    field: StateField<MarkdownTableAppearanceState>,
    from: number,
) => {
    const candidates = state.field(field, false)?.entries.filter((entry) =>
        entry.from === from || entry.from < from && entry.to >= from) ?? [];
    return candidates.find((entry) => entry.attributes.widthMode !== "auto") ?? candidates[0];
};

export const markdownTableAppearanceSnapshot = (
    state: EditorState,
    field: StateField<MarkdownTableAppearanceState>,
) => state.field(field, false)?.entries ?? [];
