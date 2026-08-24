export type SiyuanMarkdownIcon = "add" | "alignCenter" | "alignLeft" | "alignRight" | "dot" | "remove" | "table" | "trash" | "width";

const iconSymbols: Record<SiyuanMarkdownIcon, string> = {
    add: "iconAdd",
    alignCenter: "iconAlignCenter",
    alignLeft: "iconAlignLeft",
    alignRight: "iconAlignRight",
    dot: "iconDot",
    remove: "iconClose",
    table: "iconTable",
    trash: "iconTrashcan",
    width: "iconWidth",
};

export const createSiyuanMarkdownIcon = (
    ownerDocument: Document,
    name: SiyuanMarkdownIcon,
    className: string,
) => {
    const namespace = "http://www.w3.org/2000/svg";
    const svg = ownerDocument.createElementNS(namespace, "svg");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("class", className);
    const use = ownerDocument.createElementNS(namespace, "use");
    use.setAttribute("href", `#${iconSymbols[name]}`);
    svg.append(use);
    return svg;
};
