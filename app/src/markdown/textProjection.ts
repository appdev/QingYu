import {syntaxTree, syntaxTreeAvailable} from "@codemirror/language";
import {type EditorState} from "@codemirror/state";
import {type SyntaxNode, type Tree} from "@lezer/common";
import {findCodeMirrorMathRanges} from "./markra-core/codemirror/math-preview";

interface Omission {from: number; to: number; replacement?: string;}
const cache = new WeakMap<EditorState, {tree: Tree; omissions: Omission[]}>();
const inlineMarks = new Set(["EmphasisMark", "StrikethroughMark", "HighlightMark", "CodeMark"]);

const projection = (state: EditorState) => {
    const tree = syntaxTree(state);
    const cached = cache.get(state);
    if (cached?.tree === tree) return cached.omissions;
    const omissions: Omission[] = [];
    const add = (from: number, to: number, replacement = "") => {
        if (to > from) omissions.push({from, to, replacement});
    };
    const walk = (node: SyntaxNode) => {
        if (node.name === "InlineCode") {
            const marks: SyntaxNode[] = [];
            for (let child = node.firstChild; child; child = child.nextSibling) {
                if (child.name === "CodeMark") { add(child.from, child.to); marks.push(child); }
            }
            if (marks.length === 2) {
                const content = state.sliceDoc(marks[0].to, marks[1].from);
                if (content.startsWith(" ") && content.endsWith(" ") && content.trim()) {
                    add(marks[0].to, marks[0].to + 1);
                    add(marks[1].from - 1, marks[1].from);
                }
            }
            return;
        }
        if (node.name === "FencedCode" || node.name === "CodeBlock") {
            if (node.name === "FencedCode") {
                const contents: SyntaxNode[] = [];
                for (let child = node.firstChild; child; child = child.nextSibling) if (child.name === "CodeText") contents.push(child);
                if (contents.length) { add(node.from, contents[0].from); add(contents[contents.length - 1].to, node.to); }
            }
            return;
        }
        if (node.name === "Link" || node.name === "Image") {
            const marks: SyntaxNode[] = [];
            for (let child = node.firstChild; child; child = child.nextSibling) if (child.name === "LinkMark") marks.push(child);
            const closing = marks.find((mark) => state.sliceDoc(mark.from, mark.to) === "]");
            if (marks[0] && closing) {
                add(node.from, marks[0].to);
                add(closing.from, node.to);
            }
        }
        if (node.name === "Autolink") { add(node.from, node.from + 1); add(node.to - 1, node.to); return; }
        if (node.name === "Escape") { add(node.from, node.from + 1); return; }
        if (node.name === "Entity") {
            if (typeof document !== "undefined") {
                const element = document.createElement("textarea");
                element.innerHTML = state.sliceDoc(node.from, node.to);
                add(node.from, node.to, element.value);
            }
            return;
        }
        if (inlineMarks.has(node.name)) add(node.from, node.to);
        if (["HeaderMark", "QuoteMark", "ListMark"].includes(node.name)) {
            let to = node.to;
            while (to < state.doc.length && /[ \t]/u.test(state.sliceDoc(to, to + 1))) to++;
            add(node.from, to);
        }
        if (node.name === "TableDelimiter") {
            if (node.parent?.name === "Table") {
                const line = state.doc.lineAt(node.from);
                add(line.from, Math.min(state.doc.length, line.to + 1));
            }
            return;
        }
        if (node.name === "TableRow" || node.name === "TableHeader") {
            const cells: SyntaxNode[] = [];
            for (let child = node.firstChild; child; child = child.nextSibling) if (child.name === "TableCell") cells.push(child);
            if (cells.length) {
                add(node.from, cells[0].from);
                for (let i = 1; i < cells.length; i++) add(cells[i - 1].to, cells[i].from, "\t");
                add(cells[cells.length - 1].to, node.to);
            }
        }
        for (let child = node.firstChild; child; child = child.nextSibling) walk(child);
    };
    walk(tree.topNode);
    for (const math of findCodeMirrorMathRanges(state)) {
        // 公式内部不按 Markdown 强调标记处理。
        for (let i = omissions.length - 1; i >= 0; i--) {
            if (omissions[i].from >= math.from && omissions[i].to <= math.to) omissions.splice(i, 1);
        }
        const size = math.source.startsWith("$$") || math.source.startsWith("\\") ? 2 : 1;
        add(math.from, math.from + size);
        add(math.to - size, math.to);
    }
    omissions.sort((a, b) => a.from - b.from || b.to - a.to);
    cache.set(state, {tree, omissions});
    return omissions;
};

export const projectMarkdownRange = (state: EditorState, from: number, to: number): {text: string; complete: boolean} => {
    if (from < 0 || to > state.doc.length || from > to) return {text: "", complete: false};
    if (!syntaxTreeAvailable(state, to)) return {text: state.sliceDoc(from, to), complete: false};
    let cursor = from;
    let text = "";
    for (const omission of projection(state)) {
        if (omission.to <= cursor || omission.from >= to) continue;
        if (omission.from > cursor) text += state.sliceDoc(cursor, omission.from);
        // 实体等原子文本只在完整选中时解码，部分选区保留源码。
        if (omission.replacement && (omission.from < from || omission.to > to)) {
            text += state.sliceDoc(Math.max(cursor, omission.from), Math.min(to, omission.to));
        } else text += omission.replacement || "";
        cursor = Math.min(to, omission.to);
    }
    return {text: text + state.sliceDoc(cursor, to), complete: true};
};
