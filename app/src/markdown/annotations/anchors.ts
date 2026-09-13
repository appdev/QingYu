import type {AnnotationAnchor, MarkdownAnnotation} from "./types";

export const anchorAt = (text: string, from: number, to: number): AnnotationAnchor => {
    if (from < 0 || to > text.length || from >= to) throw new Error("INVALID_ANNOTATION_RANGE");
    return {from, to, quote: text.slice(from, to),
        prefix: Array.from(text.slice(Math.max(0, from - 128), from)).slice(-64).join(""),
        suffix: Array.from(text.slice(to, to + 128)).slice(0, 64).join(""), status: "attached"};
};

export const reanchor = (record: MarkdownAnnotation, text: string, trustedPositions = false): MarkdownAnnotation => {
    const matches = (from: number) => (!record.prefix || text.slice(0, from).endsWith(record.prefix)) &&
        (!record.suffix || text.slice(from + record.quote.length).startsWith(record.suffix));
    if (record.status === "attached" && text.slice(record.from, record.to) === record.quote &&
        (trustedPositions || matches(record.from))) return record;
    let match = -1;
    let count = 0;
    for (let at = text.indexOf(record.quote); at !== -1; at = text.indexOf(record.quote, at + 1)) {
        if (!matches(at)) continue;
        match = at;
        if (++count > 1) break;
    }
    return count === 1 ? {...record, ...anchorAt(text, match, match + record.quote.length)} :
        {...record, status: "orphaned"};
};
