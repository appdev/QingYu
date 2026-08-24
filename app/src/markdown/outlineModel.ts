import {getMarkdownOutlineWithPositions, type MarkdownOutlineItemWithPosition} from "./markra-core/markdown/markdown";
import {escapeHtml} from "../util/escape";

export type {MarkdownOutlineItemWithPosition};

export interface MarkdownOutlineNode extends MarkdownOutlineItemWithPosition {
    children: MarkdownOutlineNode[];
}

export const buildMarkdownOutlineTreeFromItems = (items: readonly MarkdownOutlineItemWithPosition[]): MarkdownOutlineNode[] => {
    const roots: MarkdownOutlineNode[] = [];
    const stack: MarkdownOutlineNode[] = [];
    items.forEach((item) => {
        const node: MarkdownOutlineNode = {...item, children: []};
        while (stack.length && stack[stack.length - 1].level >= node.level) stack.pop();
        (stack[stack.length - 1]?.children || roots).push(node);
        stack.push(node);
    });
    return roots;
};

export const buildMarkdownOutlineTree = (text: string): MarkdownOutlineNode[] =>
    buildMarkdownOutlineTreeFromItems(getMarkdownOutlineWithPositions(text));

const MARKDOWN_OUTLINE_ID_PREFIX = "markdown-outline:";

const toBlockTree = (node: MarkdownOutlineNode, depth: number): IBlockTree => ({
    box: "",
    nodeType: "NodeHeading",
    hPath: "",
    subType: `h${node.level}`,
    name: escapeHtml(node.title),
    type: "outline",
    depth,
    id: `${MARKDOWN_OUTLINE_ID_PREFIX}${node.from}`,
    count: node.children.length,
    children: node.children.map((child) => toBlockTree(child, depth + 1)),
});

export const buildMarkdownOutlineTreeData = (items: readonly MarkdownOutlineItemWithPosition[]): IBlockTree[] =>
    buildMarkdownOutlineTreeFromItems(items).map((node) => toBlockTree(node, 0));

export const getMarkdownOutlinePosition = (id: string | undefined) => {
    if (!id?.startsWith(MARKDOWN_OUTLINE_ID_PREFIX)) return undefined;
    const position = Number(id.slice(MARKDOWN_OUTLINE_ID_PREFIX.length));
    return Number.isFinite(position) ? position : undefined;
};
