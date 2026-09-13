import {syntaxTree, syntaxTreeAvailable} from "@codemirror/language";
import {EditorSelection, type EditorState, type ChangeSpec, type Transaction, type TransactionSpec} from "@codemirror/state";
import {type EditorView} from "@codemirror/view";
import {type SyntaxNode, type Tree} from "@lezer/common";
import {formattingPlugin} from "../markra-core/codemirror/formatting";
import {findCodeMirrorMathRanges} from "../markra-core/codemirror/math-preview";
import {resolveSafeLinkTarget} from "../markra-core/codemirror/links";
import {annotationField} from "../annotations/extension";
import {type ActionHost, type ActionResult, type FormatAction, type SelectionContext, isContextCurrent} from "./context";

interface Wrapper {from: number; start: number; end: number; to: number; marker: string; kind: string;}
const kinds: Record<string, string> = {StrongEmphasis: "bold", Emphasis: "italic", Strikethrough: "strike",
    Highlight: "highlight", InlineCode: "code", Link: "link"};
const markerNames = new Set(["EmphasisMark", "StrikethroughMark", "HighlightMark", "CodeMark"]);
const wrapperCache = new WeakMap<EditorState, {tree: Tree; wrappers: Wrapper[]}>();

export const formatWrappers = (context: SelectionContext): Wrapper[] => {
    const tree = syntaxTree(context.state);
    const cached = wrapperCache.get(context.state);
    if (cached?.tree === tree) return cached.wrappers;
    const result: Wrapper[] = [];
    tree.iterate({enter(node) {
        const kind = kinds[node.name];
        if (!kind) return;
        const marks: SyntaxNode[] = [];
        for (let child = node.node.firstChild; child; child = child.nextSibling) {
            if (kind === "link" ? child.name === "LinkMark" : markerNames.has(child.name)) marks.push(child);
        }
        const first = marks[0];
        const last = kind === "link" ? marks.find((mark) => context.state.sliceDoc(mark.from, mark.to) === "]") : marks[marks.length - 1];
        if (first && last && first !== last) {
            const content = context.state.sliceDoc(first.to, last.from);
            const padding = kind === "code" && content.startsWith(" ") && content.endsWith(" ") && content.trim() ? 1 : 0;
            result.push({from: node.from, to: node.to, start: first.to + padding, end: last.from - padding,
                marker: context.state.sliceDoc(first.from, first.to), kind});
        }
    }});
    for (const math of findCodeMirrorMathRanges(context.state)) {
        for (let i = result.length - 1; i >= 0; i--) {
            if (result[i].from >= math.from && result[i].to <= math.to) result.splice(i, 1);
        }
        const size = math.source.startsWith("$$") || math.source.startsWith("\\") ? 2 : 1;
        result.push({from: math.from, start: math.from + size, end: math.to - size, to: math.to,
            marker: math.source.slice(0, size), kind: "math"});
    }
    wrapperCache.set(context.state, {tree, wrappers: result});
    return result;
};

export const formatEnabled = (id: FormatAction, context: SelectionContext): boolean => {
    const {state, from, to} = context;
    if (context.kind !== "editor" || context.mode !== "visual" || context.view.state.readOnly || context.view.composing ||
        from === to || state.selection.ranges.length !== 1 || !syntaxTreeAvailable(state, to)) return false;
    let block: SyntaxNode = null;
    for (let node = syntaxTree(state).resolveInner(from, 1); node; node = node.parent) {
        if (["FencedCode", "CodeBlock", "Table", "HTMLBlock"].includes(node.name)) return false;
        if (node.name === "Paragraph" || /^ATXHeading/u.test(node.name)) { block = node; break; }
    }
    if (!block || to > block.to) return false;
    if (["link", "code", "math"].includes(id) && /[\r\n]/u.test(state.sliceDoc(from, to))) return false;
    const wrappers = formatWrappers(context);
    for (const wrapper of wrappers) {
        if (wrapper.from >= to || wrapper.to <= from) continue;
        if (["code", "math"].includes(wrapper.kind)) {
            const complete = from <= wrapper.start && to >= wrapper.end;
            if (!complete || (id !== wrapper.kind && id !== "clear")) return false;
        }
        if (id === "clear" && wrapper.kind === "link" && (from > wrapper.start || to < wrapper.end)) return false;
    }
    if (id === "math" && !wrappers.some((wrapper) => wrapper.kind === "math" && from <= wrapper.start && to >= wrapper.end)) {
        if (!state.sliceDoc(from, to).trim() || /[$\n\r]/u.test(state.sliceDoc(from, to))) return false;
    }
    if (id === "link") {
        const links = wrappers.filter((wrapper) => wrapper.kind === "link" && wrapper.from < to && wrapper.to > from);
        if (links.length && !(links.length === 1 && from >= links[0].start && to <= links[0].end)) return false;
    }
    return id !== "clear" || wrappers.some((wrapper) => wrapper.start < to && wrapper.end > from);
};

const publishFormat = (context: SelectionContext, tr: Transaction): ActionResult => {
    if (!isContextCurrent(context)) return "stale";
    if (context.view.state.readOnly) return "disabled";
    const records = context.view.state.field(annotationField, false) || [];
    const next = tr.state.field(annotationField, false) || [];
    if (records.some((record) => record.status === "attached" && next.find((item) => item.id === record.id)?.status !== "attached")) return "disabled";
    if (!tr.docChanged) return "disabled";
    context.view.dispatch(tr);
    context.view.focus();
    return "done";
};

const applyChanges = (context: SelectionContext, changes: ChangeSpec[]): ActionResult => {
    if (!isContextCurrent(context)) return "stale";
    const state = context.view.state;
    const unique = changes.filter((change, index) => changes.findIndex((other) => JSON.stringify(other) === JSON.stringify(change)) === index);
    const set = state.changes(unique);
    return publishFormat(context, state.update({changes: set,
        selection: EditorSelection.range(set.mapPos(context.from, 1), set.mapPos(context.to, -1)), userEvent: "input.format"}));
};

const clearWrappers = (context: SelectionContext, wrappers: Wrapper[]) => {
    const {from, to} = context;
    const changes: ChangeSpec[] = [];
    let closing = "";
    let opening = "";
    for (const wrapper of wrappers) {
        if (wrapper.start >= to || wrapper.end <= from) continue;
        if (from <= wrapper.start) changes.push({from: wrapper.from, to: wrapper.start});
        else closing = wrapper.marker + closing;
        if (to >= wrapper.end) changes.push({from: wrapper.end, to: wrapper.to});
        else opening += wrapper.marker;
    }
    if (closing) changes.push({from, insert: closing});
    if (opening) changes.push({from: to, insert: opening});
    return applyChanges(context, changes);
};

export const executeFormat = async (id: FormatAction, context: SelectionContext, host: ActionHost): Promise<ActionResult> => {
    context = {...context, state: context.view.state};
    if (!formatEnabled(id, context)) return "disabled";
    const {from, to, state} = context;
    const wrappers = formatWrappers(context);
    if (id === "link") {
        const existing = wrappers.find((wrapper) => wrapper.kind === "link" && from >= wrapper.start && to <= wrapper.end);
        let initial = "";
        if (existing) {
            let node = syntaxTree(state).resolveInner(existing.start, 1);
            while (node && node.name !== "Link") node = node.parent;
            for (let child = node?.firstChild; child; child = child.nextSibling) if (child.name === "URL") initial = state.sliceDoc(child.from, child.to);
        }
        const address = await host.requestLink(initial);
        if (address === null) return "cancelled";
        if (!host.isAlive() || !isContextCurrent(context) || host.mode() !== context.mode) return "stale";
        if (context.view.state.readOnly) return "disabled";
        const safe = resolveSafeLinkTarget(address);
        if (!safe || /[\r\n]/u.test(safe)) return "failed";
        const destination = safe.replace(/\\/gu, "\\\\").replace(/[() ]/gu, (value) => value === " " ? "%20" : `\\${value}`);
        const changes: ChangeSpec[] = [];
        if (existing) changes.push({from: existing.end, to: existing.to, insert: `](${destination})`});
        else {
            changes.push({from, insert: "["}, {from: to, insert: `](${destination})`});
            for (let i = from; i < to; i++) if (/[\[\]\\]/u.test(state.sliceDoc(i, i + 1))) changes.push({from: i, insert: "\\"});
        }
        return applyChanges(context, changes);
    }
    if (id === "clear") {
        return clearWrappers(context, wrappers);
    }
    if (id === "code" || id === "math") {
        const existing = wrappers.find((wrapper) => wrapper.kind === id && from <= wrapper.start && to >= wrapper.end);
        if (existing) return applyChanges(context, [{from: existing.from, to: existing.start}, {from: existing.end, to: existing.to}]);
        const text = state.sliceDoc(from, to);
        const marker = id === "math" ? "$" : "`".repeat(Math.max(0, ...Array.from(text.matchAll(/`+/gu), (match) => match[0].length)) + 1);
        const padding = id === "code" && (/^`|`$/u.test(text) || /^ .* $/u.test(text) && text.trim()) ? " " : "";
        return applyChanges(context, [{from, insert: marker + padding}, {from: to, insert: padding + marker}]);
    }
    const ids = {bold: "format.bold", italic: "format.italic", strike: "format.strikethrough", highlight: "format.highlight"};
    const enclosing = wrappers.find((wrapper) => wrapper.kind === id && wrapper.start <= from && wrapper.end >= to);
    if (enclosing) return clearWrappers(context, [enclosing]);
    const command = formattingPlugin().commands.find((item) => item.id === ids[id as keyof typeof ids]);
    let tr: Transaction;
    command?.run({state, dispatch: (spec: TransactionSpec) => { tr = state.update(spec); }} as unknown as EditorView);
    return tr ? publishFormat(context, tr) : "disabled";
};
