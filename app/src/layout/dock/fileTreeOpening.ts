export interface FileTreeOpening {
    started: boolean;
    finished: Promise<void>;
}

const openingItems = new WeakSet<HTMLElement>();

export const openFileTreeItem = (
    item: HTMLElement,
    open: () => void | Promise<unknown>,
): FileTreeOpening => {
    if (openingItems.has(item)) {
        return {started: false, finished: Promise.resolve()};
    }

    openingItems.add(item);
    item.setAttribute("data-opening", "true");
    const finished = Promise.resolve()
        .then(open)
        .then(() => undefined)
        .finally(() => {
            openingItems.delete(item);
            item.removeAttribute("data-opening");
        });
    return {started: true, finished};
};
