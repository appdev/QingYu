import {projectMarkdownRange} from "../textProjection";
import {initialVisualMarkdownSelection} from "../markra-core/codemirror/frontmatter-preview";
import {type SelectionContext} from "./context";

export const selectionPlainText = (context: SelectionContext): string => {
    if (context.imageRange) return projectMarkdownRange(context.state, context.from, context.to).text;
    if (context.kind !== "editor") return context.domRange?.toString() || "";
    const body = context.mode === "visual" ? initialVisualMarkdownSelection(context.state.doc.toString()) : 0;
    return context.state.selection.ranges.map((range) =>
        projectMarkdownRange(context.state, Math.min(Math.max(range.from, body), range.to), range.to).text).join("\n");
};
