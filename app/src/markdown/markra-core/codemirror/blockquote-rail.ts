import {syntaxTree} from "@codemirror/language";
import type {Extension} from "@codemirror/state";
import {layer, type EditorView, type LayerMarker, type ViewUpdate} from "@codemirror/view";
import {parseMarkdownCalloutMarker} from "../shared";
import type {MarkraSyntaxNode} from "./renderers";

const RAIL_CAP_INSET = 6;

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
        const rail = document.createElement("div");
        rail.className = "cm-markra-blockquote-rail";
        rail.dataset.from = String(this.from);
        rail.dataset.to = String(this.to);
        rail.style.setProperty("--markra-blockquote-depth", String(this.depth));
        rail.style.height = `${this.height}px`;
        rail.style.top = `${this.top}px`;
        return rail;
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
            const top = firstBlock.top + RAIL_CAP_INSET;
            const bottom = lastBlock.bottom - RAIL_CAP_INSET;
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
