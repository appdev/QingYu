import {getLuteInstance} from "../protyle/render/setLute";

let markdownHtmlLute: Lute | undefined;

const getMarkdownHtmlLute = () => {
    if (!markdownHtmlLute) {
        markdownHtmlLute = getLuteInstance() || Lute.New();
        markdownHtmlLute.SetUnorderedListMarker("-");
    }
    return markdownHtmlLute;
};

export const convertSiyuanClipboardHtmlToMarkdown = (html: string) => {
    return getMarkdownHtmlLute().HTML2Md(Lute.Sanitize(html));
};
