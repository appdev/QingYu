import {executeClipboardAction} from "./clipboard";
import {executeFormat, formatEnabled, formatWrappers} from "./format";
import {hasSelection, isContextCurrent, type ActionHost, type ActionId, type ActionResult, type EditAction, type FormatAction, type SelectionContext} from "./context";

export const editActions: EditAction[] = ["copy", "copy-plain", "cut", "delete", "paste", "paste-plain", "paste-escaped", "select-all"];
export const readActionState = (id: ActionId, context: SelectionContext): {enabled: boolean; active: boolean} => {
    if (id === "annotation") return {enabled: !context.imageRange && !context.view.state.readOnly && !context.view.composing && hasSelection(context), active: false};
    if (editActions.includes(id as EditAction)) {
        const writes = !["copy", "copy-plain", "select-all"].includes(id);
        return {enabled: !(writes && (context.view.state.readOnly || context.view.composing || context.kind === "preview")) &&
            (!(["copy", "copy-plain", "cut", "delete"].includes(id)) || hasSelection(context)), active: false};
    }
    return {enabled: formatEnabled(id as FormatAction, context), active: context.kind === "editor" &&
        formatWrappers(context).some((wrapper) => wrapper.kind === id && wrapper.start <= context.from && wrapper.end >= context.to)};
};

export const executeAction = async (id: ActionId, context: SelectionContext, host: ActionHost): Promise<ActionResult> => {
    if (!host.isAlive() || host.mode() !== context.mode || !isContextCurrent(context)) return "stale";
    if (!readActionState(id, context).enabled) return "disabled";
    let result: ActionResult;
    try {
        if (id === "annotation") result = host.addAnnotation() ? "done" : "cancelled";
        else if (editActions.includes(id as EditAction)) result = await executeClipboardAction(id as EditAction, context);
        else result = await executeFormat(id as FormatAction, context, host);
    } catch {
        result = "failed";
    }
    if (result === "failed" || result === "stale") host.report(result);
    return result;
};
