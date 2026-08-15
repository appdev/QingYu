type MarkdownTabBarCreator<T> = (app: T) => Promise<boolean>;

export const createMarkdownFromTabBarAction = <T>(
    app: T,
    target: Element,
    createMarkdown: MarkdownTabBarCreator<T>,
) => {
    const addButton = target?.closest(".block__icon[data-type=\"new\"]");
    if (!addButton) {
        return false;
    }
    void createMarkdown(app);
    return true;
};
