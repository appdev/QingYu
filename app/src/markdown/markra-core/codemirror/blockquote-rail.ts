import {syntaxTree} from "@codemirror/language";
import type {Extension} from "@codemirror/state";
import {layer, type EditorView, type LayerMarker, type ViewUpdate} from "@codemirror/view";
import {parseMarkdownCalloutMarker} from "../shared";
import type {MarkraSyntaxNode} from "./renderers";

function firstQuotedContent(view: EditorView, from: number) {
    return view.state.doc.lineAt(from).text.replace(/^[ \t]{0,3}(?:>[ \t]*)+/u, "");
}

function blockquoteDepth(node: MarkraSyntaxNode) {
    let depth = 0;
    let parent = node.parent;
    while (parent) {
        if (parent.name === "Blockquote") depth += 1;
        parent = parent.parent;
    }
    return depth;
}

function intersectsVisibleRange(view: EditorView, from: number, to: number) {
    return view.visibleRanges.some((range) => range.from <= to && range.to >= from);
}

function renderedLineBounds(view: EditorView, position: number) {
    try {
        const dom = view.domAtPos(position).node;
        const element = (dom instanceof HTMLElement ? dom : dom.parentElement)?.closest<HTMLElement>(".cm-line");
        if (!element) return null;
        const rect = element.getBoundingClientRect();
        if (rect.bottom <= rect.top) return null;
        return {
            bottom: (rect.bottom - view.documentTop) / view.scaleY,
            top: (rect.top - view.documentTop) / view.scaleY,
        };
    } catch {
        return null;
    }
}

export class BlockquoteRailMarker implements LayerMarker {
    constructor(
        readonly from: number,
        readonly to: number,
        readonly top: number,
        readonly height: number,
        readonly depth: number,
    ) {}

    eq(other: LayerMarker): boolean {
        return other instanceof BlockquoteRailMarker &&
            other.from === this.from &&
            other.to === this.to &&
            other.top === this.top &&
            other.height === this.height &&
            other.depth === this.depth;
    }

    draw() {
        const decoration = document.createElement("div");
        decoration.className = "cm-markra-blockquote-decoration";
        decoration.dataset.from = String(this.from);
        decoration.dataset.to = String(this.to);
        decoration.style.setProperty("--markra-blockquote-depth", String(this.depth));
        decoration.style.setProperty("--markra-blockquote-span-height", `${this.height}px`);
        decoration.style.setProperty("--markra-blockquote-span-top", `${this.top}px`);
        return decoration;
    }
}

function buildBlockquoteRailMarkers(view: EditorView) {
    if (view.dom.dataset.markdownMode !== "visual") return [];

    const markers: BlockquoteRailMarker[] = [];
    const seen = new Set<string>();
    syntaxTree(view.state).iterate({
        enter(node) {
            if (node.name !== "Blockquote" || !intersectsVisibleRange(view, node.from, node.to)) return;

            const key = `${node.from}:${node.to}`;
            if (seen.has(key) || parseMarkdownCalloutMarker(firstQuotedContent(view, node.from))) return;
            seen.add(key);

            const firstLine = view.state.doc.lineAt(node.from);
            const lastLine = view.state.doc.lineAt(Math.max(node.from, node.to - 1));
            const firstBlock = view.lineBlockAt(firstLine.from);
            const lastBlock = view.lineBlockAt(lastLine.from);
            const firstBounds = renderedLineBounds(view, firstLine.from);
            const lastBounds = renderedLineBounds(view, lastLine.from);
            const top = firstBounds?.top ?? firstBlock.top;
            const bottom = lastBounds?.bottom ?? lastBlock.bottom;
            markers.push(new BlockquoteRailMarker(
                node.from,
                node.to,
                top,
                Math.max(0, bottom - top),
                blockquoteDepth(node.node as MarkraSyntaxNode),
            ));
        },
    });
    return markers;
}

function blockquoteRailNeedsUpdate(update: ViewUpdate) {
    return update.docChanged || update.viewportChanged || update.geometryChanged || update.transactions.length > 0;
}

export function blockquoteRailExtension(): Extension {
    return layer({
        above: false,
        class: "cm-markra-blockquote-rail-layer",
        markers: buildBlockquoteRailMarkers,
        update: blockquoteRailNeedsUpdate,
    });
}
