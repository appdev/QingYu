import {getMarkdownLinkImageCounts, getWordCount} from "./markra-core/markdown/markdown";

export interface MarkdownStatistics {
    runeCount: number;
    wordCount: number;
    linkCount: number;
    imageCount: number;
}

export const countMarkdownStatistics = (text: string): MarkdownStatistics => {
    const {linkCount, imageCount} = getMarkdownLinkImageCounts(text);
    const countableText = text
        .replace(/(?<!\\)!\[[^\]]*\](?:\([^\n)]*\)|\[[^\n\]]*\])/gu, "")
        .replace(/(?<!\\)\[([^\]]*)\](?:\([^\n)]*\)|\[[^\n\]]*\])/gu, "$1")
        .replace(/^\s*(```|~~~).*$/gmu, "");
    return {runeCount: Array.from(text).length, wordCount: getWordCount(countableText), linkCount, imageCount};
};
