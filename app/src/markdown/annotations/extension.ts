import {EditorState, StateEffect, StateField, type Extension, type Transaction} from "@codemirror/state";
import {invertedEffects} from "@codemirror/commands";
import {syntaxTree} from "@codemirror/language";
import {Decoration, EditorView, showTooltip} from "@codemirror/view";
import {anchorAt, reanchor} from "./anchors";
import type {AnnotationAnchor, MarkdownAnnotation} from "./types";

export const changeAnnotation = StateEffect.define<MarkdownAnnotation | string>();
export const loadAnnotations = StateEffect.define<readonly MarkdownAnnotation[]>();
export const restoreAnnotationAnchors = StateEffect.define<readonly (AnnotationAnchor & {id: string})[]>({
    map: (anchors, changes) => anchors.map((anchor) => {
        const from = changes.mapPos(anchor.from, 1);
        const to = Math.max(from, changes.mapPos(anchor.to, -1));
        return {...anchor, from, to};
    }),
});

const mapRecords = (records: readonly MarkdownAnnotation[], tr: Transaction): readonly MarkdownAnnotation[] => {
    if (!tr.docChanged) return records;
    const text = tr.newDoc.toString();
    let fullReplace = false;
    tr.changes.iterChangedRanges((from, to) => {
        if (from === 0 && to === tr.startState.doc.length) fullReplace = true;
    });
    let prefix = 0;
    let suffix = 0;
    const previous = fullReplace ? tr.startState.doc.toString() : "";
    if (fullReplace) {
        while (prefix < previous.length && prefix < text.length && previous[prefix] === text[prefix]) prefix++;
        while (suffix < previous.length - prefix && suffix < text.length - prefix &&
            previous[previous.length - 1 - suffix] === text[text.length - 1 - suffix]) suffix++;
    }
    return records.map((record) => {
        if (fullReplace) {
            if (record.status === "attached" && record.to <= prefix) return {...record, ...anchorAt(text, record.from, record.to)};
            if (record.status === "attached" && record.from >= previous.length - suffix) {
                const delta = text.length - previous.length;
                return {...record, ...anchorAt(text, record.from + delta, record.to + delta)};
            }
            return reanchor(record, text);
        }
        if (record.status === "orphaned") return record;
        const from = tr.changes.mapPos(record.from, 1);
        const to = tr.changes.mapPos(record.to, -1);
        return from >= to ? {...record, from, to: from, status: "orphaned"} :
            {...record, ...anchorAt(text, from, to)};
    });
};

export const annotationField = StateField.define<readonly MarkdownAnnotation[]>({
    create: () => [],
    update(records, tr) {
        let next = mapRecords(records, tr);
        for (const effect of tr.effects) {
            if (effect.is(loadAnnotations)) next = effect.value;
            if (effect.is(changeAnnotation)) {
                const value = effect.value;
                next = typeof value === "string" ? next.filter((record) => record.id !== value) :
                    [...next.filter((record) => record.id !== value.id), value];
            }
            if (effect.is(restoreAnnotationAnchors)) {
                const anchors = new Map(effect.value.map((anchor) => [anchor.id, anchor]));
                next = next.map((record) => {
                    const anchor = anchors.get(record.id);
                    if (!anchor) return record;
                    // 历史只恢复定位；保留在表单中独立修改的笔记。
                    const {from, to, quote, prefix, suffix, status} = anchor;
                    if (status === "attached" && from < to && to <= tr.newDoc.length) {
                        return {...record, ...anchorAt(tr.newDoc.toString(), from, to)};
                    }
                    return {...record, from, to, quote, prefix, suffix, status};
                });
            }
        }
        return next;
    },
    provide: (field) => EditorView.decorations.compute([field], (state) => {
        const decorations = [];
        const lines = new Set<number>();
        for (const record of state.field(field)) {
            if (record.status !== "attached" || record.from >= record.to || record.to > state.doc.length) continue;
            decorations.push(Decoration.mark({class: "markdown-annotation-mark",
                attributes: {"data-annotation-id": record.id}}).range(record.from, record.to));
            for (let node = syntaxTree(state).resolveInner(record.from, 1); node; node = node.parent) {
                if (!/Table|FencedCode|CodeBlock|HTMLBlock|BlockHTML|MathBlock/u.test(node.name)) continue;
                const start = state.doc.lineAt(node.from).from;
                if (!lines.has(start)) decorations.push(Decoration.line({attributes: {
                    class: "markdown-annotation-block", "data-annotation-id": record.id,
                }}).range(start));
                lines.add(start);
                break;
            }
        }
        return Decoration.set(decorations, true);
    }),
});

export const annotationExtension = (initial: readonly MarkdownAnnotation[] = []): Extension => [
    annotationField.init(() => initial),
    invertedEffects.of((tr) => tr.docChanged || tr.effects.some((effect) => effect.is(restoreAnnotationAnchors))
        ? [restoreAnnotationAnchors.of(tr.startState.field(annotationField))] : []),
];

export const annotationSelectionToolbar = (label: string, add: () => void): Extension =>
    showTooltip.computeN(["selection", EditorState.readOnly], (state) => {
        const selection = state.selection.main;
        if (state.readOnly || selection.empty) return [];
        return [{pos: selection.head, above: true, strictSide: true, create: () => {
            const dom = document.createElement("div");
            dom.className = "markdown-annotation-toolbar";
            const button = document.createElement("button");
            button.type = "button";
            button.className = "b3-button b3-button--cancel";
            button.textContent = label;
            button.addEventListener("mousedown", (event) => event.preventDefault());
            button.addEventListener("click", add);
            dom.append(button);
            return {dom};
        }}];
    });
