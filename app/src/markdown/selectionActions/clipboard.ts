import {syntaxTree} from "@codemirror/language";
import {dispatchMarkdownTextPaste, escapePlainTextMarkdown} from "../markra-core/plain-text-paste";
import {selectMarkdownDocument} from "../markdownSelectAll";
import {readOptionalMarkdownHostAdapter} from "../markra-core/adapter";
import {convertCodeMirrorClipboardHtml} from "../markra-core/codemirror/html-paste";
import {selectionPlainText} from "./plainText";
import {hasSelection, isContextCurrent, restoreContextSelection, type EditAction, type ActionResult, type SelectionContext} from "./context";

const inCode = (context: SelectionContext) => {
    if (context.kind !== "editor") return Boolean(context.domRange?.startContainer.parentElement?.closest("code, pre"));
    for (let node = syntaxTree(context.state).resolveInner(context.from, 1); node; node = node.parent) {
        if (["InlineCode", "FencedCode", "CodeBlock"].includes(node.name)) return true;
    }
    return false;
};

const deleteSelection = (context: SelectionContext) => {
    restoreContextSelection(context);
    if (context.kind === "editor") {
        context.view.dispatch({...context.view.state.replaceSelection(""), userEvent: "delete.selection"});
    } else {
        const range = context.domRange.cloneRange();
        range.deleteContents();
        range.collapse(true);
        const selection = context.target.ownerDocument.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        context.target.dispatchEvent(new Event("input", {bubbles: true}));
    }
};

const copyData = (context: SelectionContext): Map<string, string> => {
    const data = new Map<string, string>();
    restoreContextSelection(context);
    const event = new Event("copy", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {value: {
        clearData: () => data.clear(), setData: (type: string, value: string) => data.set(type, value),
        getData: (type: string) => data.get(type) || "", types: [],
    }});
    context.target.dispatchEvent(event);
    if (!data.size) {
        data.set("text/plain", context.kind === "editor" ? context.state.sliceDoc(context.from, context.to) : context.domRange.toString());
        if (context.kind === "table-cell") {
            const wrapper = document.createElement("div");
            wrapper.append(context.domRange.cloneContents());
            data.set("text/html", wrapper.innerHTML);
        }
    }
    return data;
};

const writeClipboard = async (data: Map<string, string>) => {
    if (data.size === 1 && data.has("text/plain")) {
        await navigator.clipboard.writeText(data.get("text/plain"));
    } else {
        const blobs: Record<string, Blob> = {};
        for (const [type, value] of data) {
            if (type === "text/plain" || type === "text/html") blobs[type] = new Blob([value], {type});
        }
        await navigator.clipboard.write([new ClipboardItem(blobs)]);
    }
};

export const executeClipboardAction = async (action: EditAction, context: SelectionContext): Promise<ActionResult> => {
    if (!isContextCurrent(context)) return "stale";
    const writes = !["copy", "copy-plain", "select-all"].includes(action);
    if (writes && (context.view.state.readOnly || context.kind === "preview" || context.view.composing)) return "disabled";
    if (["copy", "copy-plain", "cut", "delete"].includes(action) && !hasSelection(context)) return "disabled";
    try {
        if (action === "select-all") {
            restoreContextSelection(context);
            if (context.kind === "editor") selectMarkdownDocument(context.view, context.mode);
            else {
                const range = document.createRange();
                range.selectNodeContents(context.target);
                const selection = context.target.ownerDocument.getSelection();
                selection.removeAllRanges();
                selection.addRange(range);
            }
            return "done";
        }
        if (action === "delete") { deleteSelection(context); return "done"; }
        if (["copy", "copy-plain", "cut"].includes(action)) {
            const data = action === "copy-plain" ? new Map([["text/plain", selectionPlainText(context)]]) : copyData(context);
            await writeClipboard(data);
            if (action === "cut") {
                if (!isContextCurrent(context)) return "stale";
                if (context.view.state.readOnly) return "disabled";
                deleteSelection(context);
            }
            return "done";
        }
        if (action === "paste-plain" || action === "paste-escaped") {
            const text = await navigator.clipboard.readText();
            if (!text) return "cancelled";
            if (!isContextCurrent(context)) return "stale";
            if (context.view.state.readOnly) return "disabled";
            const intent = action === "paste-escaped" && !inCode(context) ? "escaped" : "plain";
            restoreContextSelection(context);
            if (context.kind === "editor") {
                const insert = intent === "escaped" ? escapePlainTextMarkdown(text) : text;
                context.view.dispatch({...context.view.state.replaceSelection(insert), userEvent: "input.paste"});
            } else if (!dispatchMarkdownTextPaste(context.target, text, intent)) return "failed";
            return "done";
        }
        const transfer = new DataTransfer();
        for (const item of await navigator.clipboard.read()) {
            for (const type of item.types) {
                const blob = await item.getType(type);
                if (type.startsWith("text/")) transfer.setData(type, await blob.text());
                else if (type.startsWith("image/")) transfer.items.add(new File([blob], `image.${type.split("/")[1]}`, {type}));
            }
        }
        if (!isContextCurrent(context)) return "stale";
        if (context.view.state.readOnly) return "disabled";
        if (!transfer.types.length) return "cancelled";
        restoreContextSelection(context);
        const event = new ClipboardEvent("paste", {clipboardData: transfer, bubbles: true, cancelable: true});
        context.target.dispatchEvent(event);
        // 浏览器不会为合成事件执行默认粘贴；正文由 CodeMirror 消费，单元格补充文本入口。
        if (!event.defaultPrevented) {
            const plain = transfer.getData("text/plain");
            const html = transfer.getData("text/html");
            const converted = context.kind === "table-cell" && html && !inCode(context)
                ? convertCodeMirrorClipboardHtml(html, plain, readOptionalMarkdownHostAdapter(context.view.state)?.convertHtmlToMarkdown) : null;
            const text = converted?.markdown || plain;
            if (!text || context.kind === "editor") return "failed";
            if (!dispatchMarkdownTextPaste(context.target, text, "plain")) return "failed";
        }
        return "done";
    } catch {
        return "failed";
    }
};
