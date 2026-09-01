export type MarkdownExportEnvironment = "electron" | "browser" | "mobile";

export type MarkdownExportFormat = "template" | "markdownZip" | "image" | "pdf" | "html" | "docx" |
    "rst" | "asciidoc" | "textile" | "opml" | "org" | "mediawiki" | "odt" | "rtf" | "epub";

export interface MarkdownExportReference {
    notebook: string;
    path: string;
}

const pandocItems: Array<{id: string; format: MarkdownExportFormat; label: string}> = [
    {id: "exportReStructuredText", format: "rst", label: "reStructuredText"},
    {id: "exportAsciiDoc", format: "asciidoc", label: "AsciiDoc"},
    {id: "exportTextile", format: "textile", label: "Textile"},
    {id: "exportOPML", format: "opml", label: "OPML"},
    {id: "exportOrgMode", format: "org", label: "Org-Mode"},
    {id: "exportMediaWiki", format: "mediawiki", label: "MediaWiki"},
    {id: "exportODT", format: "odt", label: "ODT"},
    {id: "exportRTF", format: "rtf", label: "RTF"},
    {id: "exportEPUB", format: "epub", label: "EPUB"},
];

export const markdownExportFormats = (environment: MarkdownExportEnvironment): MarkdownExportFormat[] => {
    const formats: MarkdownExportFormat[] = ["template", "markdownZip", "image"];
    if (environment !== "browser") formats.push("pdf");
    formats.push("html");
    if (environment === "electron") formats.push("docx", ...pandocItems.map((item) => item.format));
    return formats;
};

export const currentMarkdownExportEnvironment = (): MarkdownExportEnvironment => {
    let environment: MarkdownExportEnvironment = "browser";
    /// #if !BROWSER
    environment = "electron";
    /// #endif
    /// #if MOBILE
    environment = "mobile";
    /// #endif
    return environment;
};

export const createMarkdownExportMenu = (
    reference: MarkdownExportReference,
    execute: (format: MarkdownExportFormat, reference: MarkdownExportReference) => void,
    environment = currentMarkdownExportEnvironment(),
): IMenu => {
    const formats = new Set(markdownExportFormats(environment));
    const submenu: IMenu[] = [{
        id: "exportTemplate",
        label: window.siyuan.languages.template,
        icon: "iconMarkdown",
        disabled: window.siyuan.config.readonly,
        click: () => execute("template", reference),
    }, {
        id: "exportMarkdown",
        label: "Markdown .zip",
        icon: "iconMarkdown",
        click: () => execute("markdownZip", reference),
    }, {
        id: "exportImage",
        label: window.siyuan.languages.image,
        icon: "iconImage",
        click: () => execute("image", reference),
    }];
    if (formats.has("pdf")) {
        submenu.push({
            id: "exportPDF",
            label: environment === "mobile" ? window.siyuan.languages.print : "PDF",
            icon: "iconPDF",
            click: () => execute("pdf", reference),
        });
    }
    submenu.push({
        id: "exportHTML_Markdown",
        label: "HTML (Markdown)",
        icon: "iconHTML5",
        click: () => execute("html", reference),
    });
    if (formats.has("docx")) {
        submenu.push({
            id: "exportWord",
            label: "Word .docx",
            icon: "iconDocx",
            click: () => execute("docx", reference),
        }, {
            id: "exportMore",
            label: window.siyuan.languages.more,
            icon: "iconMore",
            type: "submenu",
            submenu: pandocItems.map((item) => ({
                id: item.id,
                label: item.label,
                iconHTML: "",
                click: () => execute(item.format, reference),
            })),
        });
    }
    return {
        id: "export",
        label: window.siyuan.languages.export,
        type: "submenu",
        icon: "iconUpload",
        submenu,
    };
};

export const canExportWorkspaceMarkdown = (sourceKind: string) => sourceKind === "workspace";
