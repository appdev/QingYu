export type MarkdownMoreCommand = "toggle-typewriter" | "toggle-justify" | "toggle-rtl";

export interface MarkdownMoreMenuState {
    justify: boolean;
    rtl: boolean;
    typewriterMode: boolean;
}

export interface MarkdownMoreMenuLabels {
    justify: string;
    rtl: string;
    typewriterMode: string;
}

export const syncMarkdownModeToggle = (
    element: HTMLElement,
    preview: boolean,
    labels: {markdown: string; wysiwyg: string},
) => {
    const button = element.querySelector<HTMLElement>('[data-type="markdown-mode"]');
    button?.setAttribute("aria-label", preview ? labels.markdown : labels.wysiwyg);
    button?.querySelector("use")?.setAttribute("xlink:href", preview ? "#iconEdit" : "#iconPreview");
};

export const createMarkdownMoreMenuItems = (
    state: MarkdownMoreMenuState,
    labels: MarkdownMoreMenuLabels,
    execute: (command: MarkdownMoreCommand) => void,
    exportMenu?: IMenu,
): IMenu[] => {
    const items: IMenu[] = [{
    id: "markdownTypewriter",
    icon: "iconFocus",
    label: labels.typewriterMode,
    checked: state.typewriterMode,
    click: () => execute("toggle-typewriter"),
}, {
    id: "markdownJustify",
    icon: "iconAlignJustify",
    label: labels.justify,
    checked: state.justify,
    click: () => execute("toggle-justify"),
}, {
    id: "markdownRTL",
    icon: "iconRtl",
    label: labels.rtl,
    checked: state.rtl,
    click: () => execute("toggle-rtl"),
    }];
    if (exportMenu) items.push({id: "separator_export", type: "separator"}, exportMenu);
    return items;
};
