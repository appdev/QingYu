import type {Dialog} from "../../dialog";

interface CoverEntry {
    file: string;
    category: string;
    photographer: string;
    photographer_url: string;
    pexels_url: string;
    width: number;
    height: number;
}

function getCategoryLabel(category: string): string {
    const label = (window.siyuan.languages as Record<string, string>)[category];
    return label || category;
}

async function renderCoverPicker(dialog: Dialog, onSelect: (name: string) => void): Promise<boolean> {
    const coverData = await fetchCoverData();
    if (!coverData) {
        return false;
    }
    const {categories, coversByCategory, allCovers} = coverData;
    let activeCategory = "all";
    const buildCards = (category: string) => {
        const covers = category === "all" ? allCovers : (coversByCategory.get(category) || []);
        return covers.map((cover) => `<div class="b3-card b3-cover__card" data-name="${cover.file}"><img src="/appearance/covers/${cover.file}" loading="lazy"></div>`).join("");
    };
    const buildTabs = (category: string) => {
        let tabs = `<span class="b3-chip b3-chip--hover${category === "all" ? " b3-chip--current" : ""}" data-category="all">${window.siyuan.languages.coverAll}</span>`;
        categories.forEach((item) => {
            tabs += `<span class="b3-chip b3-chip--hover${category === item ? " b3-chip--current" : ""}" data-category="${item}">${getCategoryLabel(item)}</span>`;
        });
        return `<div class="b3-cover__tabs">${tabs}</div>`;
    };
    const bodyElement = dialog.element.querySelector(".b3-dialog__body") as HTMLElement;
    const render = () => {
        bodyElement.innerHTML = `${buildTabs(activeCategory)}
<div class="b3-cards b3-cover__cards" style="padding:16px">${buildCards(activeCategory)}</div>`;
    };
    render();
    bodyElement.addEventListener("click", (event) => {
        const target = event.target as HTMLElement;
        const categoryElement = target.closest<HTMLElement>("[data-category]");
        if (categoryElement) {
            activeCategory = categoryElement.dataset.category || "all";
            render();
            bodyElement.scrollTop = 0;
            return;
        }
        const cardElement = target.closest<HTMLElement>(".b3-cover__card");
        if (cardElement?.dataset.name) {
            onSelect(cardElement.dataset.name);
            dialog.destroy();
        }
    });
    return true;
}

let cachedCovers: CoverEntry[] | null = null;
let cachedCategories: string[] | null = null;
let cachedCoversByCategory: Map<string, CoverEntry[]> | null = null;

async function fetchCoverData(): Promise<{
    categories: string[];
    coversByCategory: Map<string, CoverEntry[]>;
    allCovers: CoverEntry[];
} | null> {
    if (cachedCovers) {
        return {
            categories: cachedCategories!,
            coversByCategory: cachedCoversByCategory!,
            allCovers: cachedCovers,
        };
    }

    try {
        const resp = await fetch("/appearance/covers/manifest.json");
        if (!resp.ok) {
            return null;
        }
        const covers: CoverEntry[] = await resp.json();

        cachedCovers = covers;
        cachedCoversByCategory = new Map();

        for (const cover of covers) {
            const list = cachedCoversByCategory.get(cover.category) || [];
            list.push(cover);
            cachedCoversByCategory.set(cover.category, list);
        }

        // 保持 manifest 中的原始顺序
        cachedCategories = [];
        const seen = new Set<string>();
        for (const cover of covers) {
            if (!seen.has(cover.category)) {
                seen.add(cover.category);
                cachedCategories.push(cover.category);
            }
        }

        return {
            categories: cachedCategories,
            coversByCategory: cachedCoversByCategory,
            allCovers: cachedCovers,
        };
    } catch (e) {
        console.warn("加载封面数据失败", e);
        return null;
    }
}

export {fetchCoverData, getCategoryLabel, renderCoverPicker, CoverEntry};
