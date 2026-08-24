import {GFM, parser as markdownParser} from "@lezer/markdown";

export const plainTextPasteMime = "application/x-markra-plain-text-paste";
const pendingProperty = "__markraPendingPlainTextPasteAt";
const intentDuration = 1500;
const joiner = "\u2060";
const parser = markdownParser.configure([GFM]);
const markerNodes = new Set(["CodeMark", "EmphasisMark", "Escape", "HeaderMark", "ListMark", "QuoteMark", "StrikethroughMark"]);

type PasteIntent = "suppress-native" | "use-native-text";
type PasteTarget = HTMLElement & {[pendingProperty]?: {markedAt: number; mode: PasteIntent}};

const punctuation = (character: string) => {
    const code = character.codePointAt(0) || 0;
    return code >= 33 && code <= 47 || code >= 58 && code <= 64 || code >= 91 && code <= 96 || code >= 123 && code <= 126;
};

export const escapePlainTextMarkdown = (text: string) => {
    const escapes = new Set<number>();
    const invisible = new Set<number>();
    for (let position = 0; position < text.length; position++) {
        if (text[position] === "\\" || text[position] === "$") escapes.add(position);
    }
    for (const match of text.matchAll(/^[ \t]*\[\^[^\]\n]+\]:/gmu)) {
        if (match.index !== undefined) invisible.add(match.index + match[0].lastIndexOf(":"));
    }
    for (const match of text.matchAll(/\[\^[^\]\n]+\]/gu)) {
        if (match.index === undefined) continue;
        invisible.add(match.index + 1);
        escapes.add(match.index + match[0].length - 1);
    }
    const cursor = parser.parse(text).cursor();
    do {
        const source = text.slice(cursor.from, cursor.to);
        if (markerNodes.has(cursor.name) || cursor.name === "HorizontalRule" || cursor.name === "TableDelimiter") {
            for (let position = cursor.from; position < cursor.to; position++) {
                if (punctuation(text[position] || "")) escapes.add(position);
            }
        } else if (cursor.name === "LinkReference") {
            const definitionColon = source.indexOf("]:");
            if (definitionColon >= 0) {
                escapes.add(cursor.from + definitionColon);
                invisible.add(cursor.from + definitionColon + 1);
            }
        } else if (cursor.name === "CodeBlock") {
            invisible.add(text.lastIndexOf("\n", cursor.from - 1) + 1);
        } else if (cursor.name === "LinkMark") {
            for (let offset = 0; offset < source.length; offset++) {
                if (source[offset] === "]" || source[offset] === "!" || source[offset] === "<") escapes.add(cursor.from + offset);
            }
        } else if (cursor.name === "TaskMarker") {
            const bracket = source.lastIndexOf("]");
            if (bracket >= 0) escapes.add(cursor.from + bracket);
        } else if (cursor.name === "HTMLTag" || cursor.name === "Entity") {
            escapes.add(cursor.from);
        } else if (cursor.name === "URL") {
            const separator = source.includes(":") ? source.indexOf(":") : source.indexOf("@");
            if (separator >= 0) escapes.add(cursor.from + separator);
        }
    } while (cursor.next());
    return [...Array.from(escapes, (position) => ({position, prefix: "\\"})),
        ...Array.from(invisible, (position) => ({position, prefix: joiner}))]
        .sort((left, right) => right.position - left.position)
        .reduce((result, item) => `${result.slice(0, item.position)}${item.prefix}${result.slice(item.position)}`, text)
        .replaceAll("==", `=${joiner}=`);
};

const clipboardData = (text: string) => ({
    files: Object.assign([], {item: () => null}),
    getData: (type: string) => type === plainTextPasteMime ? "true" : type === "text/plain" ? escapePlainTextMarkdown(text) : "",
    types: [plainTextPasteMime, "text/plain"],
});

export const isPlainTextPaste = (event: ClipboardEvent) => event.clipboardData?.getData(plainTextPasteMime) === "true";

export const markNextPlainTextPaste = (target: HTMLElement, mode: PasteIntent = "suppress-native") => {
    (target as PasteTarget)[pendingProperty] = {markedAt: Date.now(), mode};
};

export const consumeNextPlainTextPaste = (target: HTMLElement): PasteIntent | null => {
    const pasteTarget = target as PasteTarget;
    const pending = pasteTarget[pendingProperty];
    delete pasteTarget[pendingProperty];
    return pending && Date.now() - pending.markedAt <= intentDuration ? pending.mode : null;
};

const selectedEditable = (target: HTMLElement) => {
    const selectionNode = target.classList.contains("cm-content") ? target.ownerDocument.getSelection()?.anchorNode : null;
    const origin = selectionNode instanceof Element ? selectionNode : selectionNode?.parentElement;
    return (origin && target.contains(origin) ? origin : target).closest<HTMLElement>("input, textarea, [contenteditable]");
};

export const dispatchPlainTextPaste = (target: HTMLElement, text: string): boolean => {
    const editable = selectedEditable(target);
    if (editable instanceof HTMLInputElement || editable instanceof HTMLTextAreaElement) {
        if (editable.disabled) return false;
        editable.setRangeText(text, editable.selectionStart || 0, editable.selectionEnd || editable.selectionStart || 0, "end");
        editable.dispatchEvent(new Event("input", {bubbles: true}));
        return true;
    }
    if (editable && !editable.classList.contains("cm-content")) {
        const selection = editable.ownerDocument.getSelection();
        if (!selection?.rangeCount) return false;
        const range = selection.getRangeAt(0).cloneRange();
        const start = (range.startContainer instanceof Element ? range.startContainer : range.startContainer.parentElement)?.closest("td, th");
        const end = (range.endContainer instanceof Element ? range.endContainer : range.endContainer.parentElement)?.closest("td, th");
        if (start && end && start !== end) range.collapse(true);
        range.deleteContents();
        const fragment = editable.ownerDocument.createDocumentFragment();
        let lastNode: Node | null = null;
        escapePlainTextMarkdown(text).split(/\r\n?|\n/u).forEach((line, index) => {
            if (index) {
                const lineBreak = editable.ownerDocument.createElement("br");
                lineBreak.dataset.markraSourceBreak = "true";
                fragment.append(lineBreak);
                lastNode = lineBreak;
            }
            const textNode = editable.ownerDocument.createTextNode(line);
            fragment.append(textNode);
            lastNode = textNode;
        });
        range.insertNode(fragment);
        if (lastNode) range.setStartAfter(lastNode);
        range.collapse(true);
        selection.removeAllRanges();
        selection.addRange(range);
        editable.dispatchEvent(new Event("input", {bubbles: true}));
        return true;
    }
    if (!target.isConnected) return false;
    const event = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {value: clipboardData(text)});
    target.dispatchEvent(event);
    return event.defaultPrevented;
};

export const handlePendingPlainTextPasteEvent = (event: ClipboardEvent, target: HTMLElement) => {
    const intent = consumeNextPlainTextPaste(target);
    if (!intent) return false;
    event.preventDefault();
    if (intent === "use-native-text") {
        const text = event.clipboardData?.getData("text/plain") || "";
        if (text) dispatchPlainTextPaste(event.target instanceof HTMLElement && target.contains(event.target) ? event.target : target, text);
    }
    return true;
};
