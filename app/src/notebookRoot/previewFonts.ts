// 无法读取跨域样式表时不复用字体，避免遗漏主题中的字体变更。
export const documentCardPreviewFontSignature = (): string | undefined => {
    const sheets: unknown[] = [];
    const fonts = new Set<string>();
    const visit = (sheet: CSSStyleSheet, ancestors: Set<CSSStyleSheet>) => {
        if (ancestors.has(sheet)) return;
        const next = new Set(ancestors).add(sheet);
        const rules = (list: CSSRuleList) => Array.from(list).forEach((rule) => {
            // 截图库会将导入字体复制到宿主样式表，同一声明不应改变缓存标识。
            if (rule.type === 5) fonts.add(rule.cssText);
            const imported = (rule as CSSImportRule).styleSheet;
            if (imported) visit(imported, next);
            const children = (rule as CSSGroupingRule).cssRules;
            if (children) {
                fonts.add((rule as CSSConditionRule).conditionText || "");
                rules(children);
            }
        });
        rules(sheet.cssRules);
        sheets.push([sheet.href, sheet.disabled, sheet.media?.mediaText]);
    };
    try {
        Array.from(document.styleSheets).forEach((sheet) => visit(sheet, new Set()));
        return JSON.stringify([sheets, Array.from(fonts)]);
    } catch {
        return undefined;
    }
};

export class DocumentCardPreviewFontCache {
    private entry?: {key: string, value: Promise<string>};

    public get(key: string | undefined, load: () => Promise<string>) {
        if (key === undefined) return load();
        if (this.entry?.key === key) return this.entry.value;
        const entry = {key, value: Promise.resolve().then(load)};
        this.entry = entry;
        void entry.value.catch(() => {
            if (this.entry === entry) this.entry = undefined;
        });
        return entry.value;
    }
}

const fontCache = new DocumentCardPreviewFontCache();

export const documentCardPreviewFontCSS = async (element: HTMLElement, appearance: string) => {
    const signature = documentCardPreviewFontSignature();
    // 不同卡片可能使用不同字体族，不能沿用仅覆盖前一张卡片的字体样式。
    const families = Array.from(new Set([element, ...Array.from(element.querySelectorAll<HTMLElement>("*"))]
        .map((node) => window.getComputedStyle(node).fontFamily))).sort();
    const key = signature === undefined ? undefined : JSON.stringify([appearance, signature, families]);
    const css = await fontCache.get(key, () => window.htmlToImage.getFontEmbedCSS(element, {includeQueryParams: true}));
    const {subsetDocumentCardPreviewFonts} = await import("./previewFontSubset");
    return subsetDocumentCardPreviewFonts(element, css);
};
