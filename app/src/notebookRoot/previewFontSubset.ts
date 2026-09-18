export const hasSubsetCandidate = (css: string): boolean => {
    for (const match of css.matchAll(/data:[^\s"'()]*?;base64,/g)) {
        const start = match.index + match[0].length;
        const end = css.indexOf(")", start);
        if (end - start < 350000) continue;
        try {
            const header = atob(css.slice(start, start + 4096));
            const woff = header.slice(0, 4) === "wOFF";
            if (!woff && header.slice(0, 4) !== "\0\x01\0\0") continue;
            const countOffset = woff ? 12 : 4;
            const count = header.charCodeAt(countOffset) * 256 + header.charCodeAt(countOffset + 1);
            const offset = woff ? 44 : 12;
            const stride = woff ? 20 : 16;
            if (count > 128 || offset + count * stride > header.length) continue;
            const tags = new Set<string>();
            for (let index = 0; index < count; index++) tags.add(header.slice(offset + index * stride, offset + index * stride + 4));
            if (tags.has("glyf") && !["COLR", "CPAL", "CBDT", "CBLC", "sbix", "SVG ", "fvar"].some((tag) => tags.has(tag))) {
                return true;
            }
        } catch {
            // 无法判断时沿用完整字体。
        }
    }
    return false;
};

export const documentCardPreviewCharacters = (element: HTMLElement): string | undefined => {
    let text = (element.textContent || "") + (element.innerText || "");
    // 标准列表标记可能由浏览器生成，不在文本节点中。
    text += "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.•◦▪()";
    for (const node of [element, ...Array.from(element.querySelectorAll<HTMLElement>("*"))]) {
        const style = window.getComputedStyle(node);
        if (style.display === "list-item" && !["none", "disc", "circle", "square", "decimal", "decimal-leading-zero",
            "lower-alpha", "upper-alpha", "lower-latin", "upper-latin", "lower-roman", "upper-roman"].includes(style.listStyleType)) {
            return undefined;
        }
        for (const pseudo of ["::before", "::after", "::marker"]) {
            const pseudoStyle = window.getComputedStyle(node, pseudo);
            const content = pseudoStyle.content;
            if (!content || content === "none" || content === "normal") continue;
            if (pseudoStyle.textTransform && pseudoStyle.textTransform !== "none") return undefined;
            if (!/^"[^"\\]*"$/.test(content)) return undefined;
            text += content.slice(1, -1);
        }
        if (text.length > 65536) return undefined;
    }
    return Array.from(new Set(text)).sort().join("");
};

export class PreviewFontSubsetClient {
    private nextID = 0;
    private result?: {source: string, text: string, css: string};
    private pending = new Map<number, {resolve: (css?: string) => void, timer: number}>();

    public stop() {
        this.pending.forEach(({resolve, timer}) => {
            window.clearTimeout(timer);
            resolve();
        });
        this.pending.clear();
    }

    public async subset(css: string, text: string): Promise<string> {
        if (this.result?.source === css && this.result.text === text) return this.result.css;
        if (css.length > 64 * 1024 * 1024 || text.length > 16384 || typeof window.require !== "function" ||
            !hasSubsetCandidate(css)) return css;
        try {
            const {ipcRenderer} = window.require("electron") as typeof import("electron");
            const id = ++this.nextID;
            const result = await new Promise<string | undefined>((resolve) => {
                const finish = (value?: string) => {
                    const pending = this.pending.get(id);
                    if (!pending) return;
                    this.pending.delete(id);
                    window.clearTimeout(pending.timer);
                    resolve(typeof value === "string" ? value : undefined);
                };
                const timer = window.setTimeout(() => finish(), 5500);
                this.pending.set(id, {resolve, timer});
                void ipcRenderer.invoke("siyuan-preview-font-subset", {css, text}).then(finish, () => finish());
            });
            if (result !== undefined) this.result = {source: css, text, css: result};
            return result ?? css;
        } catch {
            this.stop();
            return css;
        }
    }
}

const client = new PreviewFontSubsetClient();
if (typeof window !== "undefined") window.addEventListener("beforeunload", () => client.stop());

export const subsetDocumentCardPreviewFonts = (element: HTMLElement, css: string) => {
    if (typeof window.require !== "function" || !hasSubsetCandidate(css)) return Promise.resolve(css);
    const text = documentCardPreviewCharacters(element);
    return text === undefined ? Promise.resolve(css) : client.subset(css, text);
};
