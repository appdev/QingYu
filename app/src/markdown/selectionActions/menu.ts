import {editActions, executeAction, readActionState} from "./commands";
import {type ActionHost, type ActionId, type SelectionContext} from "./context";

export const actionPresentation = (id: ActionId): {label: string; icon: string} => {
    const lang = window.siyuan.languages;
    const labels: Record<ActionId, string> = {copy: lang.copy, "copy-plain": lang.copyPlainText, cut: lang.cut,
        delete: lang.delete, paste: lang.paste, "paste-plain": lang.pasteAsPlainText, "paste-escaped": lang.pasteEscaped,
        "select-all": lang.selectAll, link: lang.link, bold: lang.bold, italic: lang.italic, strike: lang.strike,
        highlight: lang.mark, code: lang.replaceTypes.code, math: lang.replaceTypes.inlineMath,
        clear: lang.clearFontStyle, annotation: lang.markdownAnnotationAdd};
    const icons: Record<ActionId, string> = {copy: "iconCopy", "copy-plain": "iconCopy", cut: "iconCut", delete: "iconTrashcan",
        paste: "iconPaste", "paste-plain": "iconPaste", "paste-escaped": "iconPaste", "select-all": "iconSelectAll",
        link: "iconLink", bold: "iconBold", italic: "iconItalic", strike: "iconStrike", highlight: "iconMark",
        code: "iconInlineCode", math: "iconMath", clear: "iconClear", annotation: "iconMark"};
    return {label: labels[id], icon: icons[id]};
};

export const createSelectionMenu = (context: SelectionContext, host: ActionHost): IMenu[] => {
    const items: IMenu[] = [];
    for (const id of editActions) {
        if (id === "paste" || id === "select-all") items.push({type: "separator"});
        items.push({id: `markdown-${id}`, ...actionPresentation(id), disabled: !readActionState(id, context).enabled,
            click: () => { void executeAction(id, context, host); }});
    }
    return items;
};
