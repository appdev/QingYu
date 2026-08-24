export const syncMarkdownFullscreenButton = (element: Element, active: boolean) => {
    element.querySelector('[data-type="markdown-fullscreen"] use')
        ?.setAttribute("xlink:href", active ? "#iconFullscreenExit" : "#iconFullscreen");
};

export const syncMarkdownFullscreenModels = (
    editors: readonly {element: HTMLElement}[],
    activeElement: Element,
    enabled: boolean,
) => {
    editors.forEach((editor) => {
        const active = editor.element === activeElement && enabled;
        editor.element.classList.toggle("fullscreen", active);
        syncMarkdownFullscreenButton(editor.element, active);
    });
};
