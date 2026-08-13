export interface CodeLanguageMenuHandle {
    element: HTMLElement;
    focus(): void;
    destroy(): void;
}

export interface CodeLanguageFilterDetail {
    languages: string[];
    listElement: HTMLElement;
    type: "init" | "match";
    value: string;
}

export interface CodeLanguageMenuOptions {
    anchor: HTMLElement;
    container: HTMLElement;
    currentLanguage: string;
    languages: readonly string[];
    labels: {clear: string; search: string};
    element?: HTMLElement;
    onDestroy?(): void;
    onFilter?(detail: CodeLanguageFilterDetail): readonly string[];
    onSelect(language: string): void;
    position(anchor: HTMLElement, popover: HTMLElement): void;
}

const normalizeLanguages = (languages: readonly unknown[]) => [...new Set(languages
    .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    .map((item) => item.trim()))].sort((left, right) => left.localeCompare(right));

const filterLanguages = (languages: readonly string[], value: string) => {
    if (!value) return [...languages];
    const query = value.toLowerCase();
    return languages.filter((language) => language.toLowerCase().includes(query)).sort((left, right) => {
        const leftStarts = left.toLowerCase().startsWith(query);
        const rightStarts = right.toLowerCase().startsWith(query);
        if (leftStarts !== rightStarts) return leftStarts ? -1 : 1;
        if (leftStarts && rightStarts && left.length !== right.length) return left.length - right.length;
        return left.localeCompare(right);
    });
};

export const mountCodeLanguageMenu = (options: CodeLanguageMenuOptions): CodeLanguageMenuHandle => {
    const document = options.anchor.ownerDocument;
    const ownsElement = !options.element;
    const element = options.element ?? document.createElement("div");
    const content = document.createElement("div");
    const search = document.createElement("input");
    const list = document.createElement("div");
    let destroyed = false;
    let languages = normalizeLanguages(options.languages);

    element.classList.add("protyle-util", "markra-code-language-popover");
    element.classList.remove("fn__none");
    element.replaceChildren();
    content.className = "fn__flex-column";
    content.dataset.id = "codeLanguage";
    content.style.maxHeight = "50vh";
    search.className = "b3-text-field";
    search.placeholder = options.labels.search;
    search.style.margin = "0 8px 4px 8px";
    list.className = "b3-list fn__flex-1 b3-list--background";
    list.style.position = "relative";
    content.append(search, list);
    element.append(content);
    if (ownsElement) options.container.append(element);

    const applyFilter = (items: readonly string[], type: CodeLanguageFilterDetail["type"], value: string) => {
        const normalized = normalizeLanguages(items);
        if (!options.onFilter) return normalized;
        try {
            return normalizeLanguages(options.onFilter({
                languages: [...normalized],
                listElement: list,
                type,
                value,
            }));
        } catch {
            return normalized;
        }
    };

    languages = applyFilter(languages, "init", "");

    const focusedItem = () => list.querySelector<HTMLElement>(".b3-list-item--focus");

    const setFocused = (item: HTMLElement | null) => {
        focusedItem()?.classList.remove("b3-list-item--focus");
        item?.classList.add("b3-list-item--focus");
        item?.scrollIntoView?.({block: "nearest"});
    };

    const destroy = () => {
        if (destroyed) return;
        destroyed = true;
        document.removeEventListener("mousedown", onOutsidePointer, true);
        if (ownsElement) {
            element.remove();
        } else {
            element.replaceChildren();
            element.classList.add("fn__none");
        }
        options.onDestroy?.();
    };

    const select = (item: HTMLElement) => {
        const language = item.dataset.id === "clearLanguage"
            ? ""
            : item.dataset.id === "customLanguage"
                ? item.textContent ?? ""
                : item.dataset.id ?? "";
        destroy();
        options.onSelect(language);
    };

    const render = (items: readonly string[], value: string) => {
        list.replaceChildren();
        const clear = document.createElement("div");
        clear.className = "b3-list-item";
        clear.dataset.id = "clearLanguage";
        clear.textContent = options.labels.clear;
        list.append(clear);
        for (const language of items) {
            const option = document.createElement("div");
            option.className = "b3-list-item";
            option.dataset.id = language;
            option.textContent = language;
            list.append(option);
        }
        const customLanguage = value.replace(/`| /gu, "_");
        if (customLanguage && !items.includes(customLanguage)) {
            const custom = document.createElement("div");
            const strong = document.createElement("b");
            custom.className = "b3-list-item";
            custom.dataset.id = "customLanguage";
            strong.textContent = customLanguage;
            custom.append(strong);
            list.append(custom);
        }
        const exact = Array.from(list.querySelectorAll<HTMLElement>(".b3-list-item"))
            .find((item) => item.dataset.id === (value || options.currentLanguage));
        setFocused(exact ?? list.children[1] as HTMLElement | null ?? clear);
    };

    const onOutsidePointer = (event: MouseEvent) => {
        const target = event.target as Node;
        if (!element.contains(target) && !options.anchor.contains(target)) destroy();
    };

    search.addEventListener("input", (event) => {
        const value = search.value.trim();
        const matches = applyFilter(filterLanguages(languages, value), "match", value);
        render(matches, value);
        event.stopPropagation();
    });
    search.addEventListener("keydown", (event) => {
        if (event.isComposing) return;
        const items = Array.from(list.querySelectorAll<HTMLElement>(".b3-list-item"));
        const current = focusedItem();
        if (event.key === "Escape") {
            destroy();
            options.anchor.focus();
            event.preventDefault();
        } else if (event.key === "Enter" && current) {
            select(current);
            event.preventDefault();
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            const index = Math.max(0, items.indexOf(current as HTMLElement));
            const next = event.key === "ArrowDown"
                ? Math.min(items.length - 1, index + 1)
                : Math.max(0, index - 1);
            setFocused(items[next] ?? null);
            event.preventDefault();
        }
        event.stopPropagation();
    });
    list.addEventListener("click", (event) => {
        const item = (event.target as HTMLElement).closest<HTMLElement>(".b3-list-item");
        if (!item) return;
        select(item);
        event.preventDefault();
        event.stopPropagation();
    });
    document.addEventListener("mousedown", onOutsidePointer, true);
    render(languages, "");
    options.position(options.anchor, element);

    return {
        element,
        focus: () => search.select(),
        destroy,
    };
};
