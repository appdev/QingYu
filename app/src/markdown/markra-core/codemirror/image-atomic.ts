import {syntaxTree} from "@codemirror/language";
import {
    Annotation,
    EditorSelection,
    EditorState,
    Prec,
    StateEffect,
    StateField,
    Transaction,
    type ChangeSpec,
    type TransactionSpec,
} from "@codemirror/state";
import {EditorView, keymap} from "@codemirror/view";
import {imageAttributeDetails, imageAttributeListLength} from "./image-attributes";
import {defineMarkraPlugin} from "./plugin";

export interface ImageAtomicRange {
    readonly from: number;
    readonly to: number;
}

const imageAtomicEdit = Annotation.define<boolean>();
const selectImageEffect = StateEffect.define<ImageAtomicRange | null>();

const sameRange = (left: ImageAtomicRange | null, right: ImageAtomicRange | null) =>
    left?.from === right?.from && left?.to === right?.to;

const imageAtomicSelection = StateField.define<ImageAtomicRange | null>({
    create: () => null,
    update(selected, transaction) {
        let next = transaction.docChanged ? null : selected;
        for (const effect of transaction.effects) {
            if (effect.is(selectImageEffect)) {
                next = effect.value;
            }
        }
        return next;
    },
});

export const readImageAtomicRanges = (state: EditorState) => {
    const ranges: ImageAtomicRange[] = [];
    syntaxTree(state).iterate({
        enter(node) {
            if (node.name !== "Image") {
                return;
            }
            const attributes = imageAttributeDetails(state, node.node);
            const line = state.doc.lineAt(node.to);
            const fallbackAttributeLength = attributes.ownedTo === node.to
                ? imageAttributeListLength(state.sliceDoc(node.to, line.to))
                : null;
            ranges.push({
                from: node.from,
                to: fallbackAttributeLength === null ? attributes.ownedTo : node.to + fallbackAttributeLength,
            });
        },
    });
    return ranges;
};

export const getSelectedImageAtomicRange = (state: EditorState) =>
    state.field(imageAtomicSelection, false) ?? null;

export const selectImageAtomicRange = (view: EditorView, range: ImageAtomicRange) => {
    if (view.state.facet(EditorState.readOnly)) {
        return;
    }
    view.dispatch({effects: selectImageEffect.of(range)});
};

export const clearImageAtomicSelection = (view: EditorView) => {
    if (!getSelectedImageAtomicRange(view.state)) {
        return false;
    }
    view.dispatch({effects: selectImageEffect.of(null)});
    return true;
};

const adjacentImageRange = (state: EditorState, direction: "backward" | "forward") => {
    const ranges = state.selection.ranges;
    if (ranges.some((range) => !range.empty)) {
        return null;
    }
    const adjacent = ranges.map((selection) => readImageAtomicRanges(state).find((range) =>
        direction === "backward"
            ? range.from < selection.head && selection.head <= range.to
            : range.from <= selection.head && selection.head < range.to));
    return adjacent.every(Boolean) ? adjacent as ImageAtomicRange[] : null;
};

const mergeRanges = (ranges: readonly ImageAtomicRange[]) => {
    const sorted = [...ranges].sort((left, right) => left.from - right.from || left.to - right.to);
    const merged: ImageAtomicRange[] = [];
    for (const range of sorted) {
        const previous = merged[merged.length - 1];
        if (previous && range.from <= previous.to) {
            merged[merged.length - 1] = {from: previous.from, to: Math.max(previous.to, range.to)};
        } else {
            merged.push(range);
        }
    }
    return merged;
};

const deleteImageRanges = (view: EditorView, ranges: readonly ImageAtomicRange[], direction: "backward" | "forward") => {
    const merged = mergeRanges(ranges);
    if (merged.length === 0) {
        return false;
    }
    view.dispatch({
        annotations: [imageAtomicEdit.of(true), Transaction.userEvent.of(`delete.${direction}`)],
        changes: merged.map(({from, to}) => ({from, to})),
        selection: EditorSelection.cursor(merged[0].from),
    });
    return true;
};

const selectOrDeleteAdjacentImage = (view: EditorView, direction: "backward" | "forward") => {
    if (view.state.facet(EditorState.readOnly) || view.composing) {
        return false;
    }
    const ranges = adjacentImageRange(view.state, direction);
    if (!ranges || ranges.length === 0) {
        return false;
    }
    if (ranges.length > 1) {
        return deleteImageRanges(view, ranges, direction);
    }
    const selected = getSelectedImageAtomicRange(view.state);
    if (sameRange(selected, ranges[0])) {
        return deleteImageRanges(view, ranges, direction);
    }
    view.dispatch({effects: selectImageEffect.of(ranges[0])});
    return true;
};

const copySelectedImage = (event: ClipboardEvent, view: EditorView, cut: boolean) => {
    const selected = getSelectedImageAtomicRange(view.state);
    if (!selected || !event.clipboardData) {
        return false;
    }
    event.preventDefault();
    event.clipboardData.clearData();
    event.clipboardData.setData("text/plain", view.state.sliceDoc(selected.from, selected.to));
    if (cut && !view.state.facet(EditorState.readOnly)) {
        deleteImageRanges(view, [selected], "backward");
    }
    return true;
};

const handleImageBeforeInput = (event: InputEvent, view: EditorView) => {
    if (event.isComposing) {
        return false;
    }
    const direction = event.inputType === "deleteContentBackward"
        ? "backward"
        : event.inputType === "deleteContentForward" ? "forward" : null;
    if (!direction || !selectOrDeleteAdjacentImage(view, direction)) {
        return false;
    }
    event.preventDefault();
    return true;
};

interface NormalizedChange {
    from: number;
    insert: string;
    to: number;
}

const normalizeImageAtomicTransaction = (transaction: Transaction): TransactionSpec | null => {
    if (!transaction.docChanged || transaction.annotation(imageAtomicEdit)) {
        return null;
    }
    const atomicRanges = readImageAtomicRanges(transaction.startState);
    const changes: NormalizedChange[] = [];
    let expanded = false;
    transaction.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
        let from = fromA;
        let to = toA;
        for (const range of atomicRanges) {
            if (fromA < range.to && toA > range.from && (fromA > range.from || toA < range.to)) {
                from = Math.min(from, range.from);
                to = Math.max(to, range.to);
                expanded = true;
            }
        }
        changes.push({from, insert: inserted.toString(), to});
    });
    if (!expanded) {
        return null;
    }
    const merged: ChangeSpec[] = [];
    for (const change of changes.sort((left, right) => left.from - right.from)) {
        const previous = merged[merged.length - 1] as NormalizedChange | undefined;
        if (previous && change.from <= previous.to) {
            previous.to = Math.max(previous.to, change.to);
            if (!previous.insert) {
                previous.insert = change.insert;
            }
        } else {
            merged.push({...change});
        }
    }
    const first = merged[0] as NormalizedChange;
    return {
        annotations: [imageAtomicEdit.of(true), Transaction.userEvent.of("delete")],
        changes: merged,
        selection: EditorSelection.cursor(first.from),
    };
};

export const imageAtomicEditingPlugin = () => defineMarkraPlugin({
    id: "markra.image-atomic-editing",
    extension: [
        imageAtomicSelection,
        EditorState.transactionFilter.of((transaction) => normalizeImageAtomicTransaction(transaction) ?? transaction),
        Prec.highest(keymap.of([
            {key: "Backspace", run: (view) => selectOrDeleteAdjacentImage(view, "backward")},
            {key: "Delete", run: (view) => selectOrDeleteAdjacentImage(view, "forward")},
            {key: "Escape", run: clearImageAtomicSelection},
        ])),
        EditorView.domEventHandlers({
            beforeinput: handleImageBeforeInput,
            copy: (event, view) => copySelectedImage(event, view, false),
            cut: (event, view) => copySelectedImage(event, view, true),
        }),
    ],
});
