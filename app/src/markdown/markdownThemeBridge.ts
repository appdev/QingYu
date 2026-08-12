const bridges = new WeakMap<Document, MarkdownThemeBridge>();

const STYLE_PROPERTIES = [
    "backgroundColor",
    "borderBottomColor",
    "borderBottomStyle",
    "borderBottomWidth",
    "borderLeftColor",
    "borderLeftStyle",
    "borderLeftWidth",
    "borderRadius",
    "color",
    "fontFamily",
    "fontSize",
    "fontStyle",
    "fontWeight",
    "letterSpacing",
    "lineHeight",
    "textDecorationColor",
    "textDecorationLine",
    "textDecorationStyle",
] as const;

const PROBES = [
    ["root", ".markdown-theme-probe__paragraph"],
    ["h1", ".markdown-theme-probe__h1"],
    ["h2", ".markdown-theme-probe__h2"],
    ["h3", ".markdown-theme-probe__h3"],
    ["h4", ".markdown-theme-probe__h4"],
    ["h5", ".markdown-theme-probe__h5"],
    ["h6", ".markdown-theme-probe__h6"],
    ["blockquote", ".markdown-theme-probe__blockquote"],
    ["code", ".markdown-theme-probe__code .hljs"],
    ["strong", '[data-markdown-theme-probe="strong"]'],
    ["emphasis", '[data-markdown-theme-probe="emphasis"]'],
    ["strikethrough", '[data-markdown-theme-probe="strikethrough"]'],
    ["highlight", '[data-markdown-theme-probe="highlight"]'],
    ["inline-code", '[data-markdown-theme-probe="inline-code"]'],
    ["link", '[data-markdown-theme-probe="link"]'],
    ["table", ".markdown-theme-probe__table"],
    ["table-head", ".markdown-theme-probe__table th"],
    ["table-cell", ".markdown-theme-probe__table td"],
] as const;

const toKebabCase = (value: string) => value.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);

class MarkdownThemeBridge {
    private document: Document;
    private window: Window;
    private probe: HTMLElement;
    private observer: MutationObserver;
    private references = 0;
    private frame = 0;
    private disposed = false;
    private variables = new Set<string>();
    private onLoad = (event: Event) => {
        if (["LINK", "STYLE"].includes((event.target as Element | null)?.tagName)) {
            this.scheduleRefresh();
        }
    };

    constructor(document: Document) {
        this.document = document;
        this.window = document.defaultView;
        this.probe = this.createProbe();
        document.body.append(this.probe);
        this.observer = new MutationObserver(() => this.scheduleRefresh());
        this.observer.observe(document.documentElement, {
            attributeFilter: ["class", "data-dark-theme", "data-light-theme", "data-theme-mode"],
            attributes: true,
        });
        this.observer.observe(document.head, {
            attributeFilter: ["disabled", "href", "media"],
            attributes: true,
            characterData: true,
            childList: true,
            subtree: true,
        });
        document.addEventListener("load", this.onLoad, true);
        this.refresh();
    }

    public acquire() {
        this.references += 1;
    }

    public release() {
        this.references -= 1;
        if (this.references > 0) {
            return;
        }
        this.disposed = true;
        this.observer.disconnect();
        this.document.removeEventListener("load", this.onLoad, true);
        this.window.cancelAnimationFrame(this.frame);
        this.probe.remove();
        this.variables.forEach((name) => this.document.documentElement.style.removeProperty(name));
        bridges.delete(this.document);
    }

    public refresh() {
        if (this.disposed) {
            return;
        }
        const style = this.document.documentElement.style;
        PROBES.forEach(([name, selector]) => {
            const element = this.probe.querySelector<HTMLElement>(selector);
            if (!element) {
                return;
            }
            const computed = this.window.getComputedStyle(element);
            STYLE_PROPERTIES.forEach((property) => {
                const value = computed[property];
                if (!value) {
                    return;
                }
                const variable = `--b3-markdown-${name}-${toKebabCase(property)}`;
                style.setProperty(variable, value);
                this.variables.add(variable);
            });
        });
        const quote = this.probe.querySelector<HTMLElement>(".markdown-theme-probe__blockquote");
        if (quote && !this.window.navigator.userAgent.includes("jsdom")) {
            try {
                const before = this.window.getComputedStyle(quote, "::before");
                [["background-color", before.backgroundColor], ["border-radius", before.borderRadius], ["width", before.width]].forEach(([property, value]) => {
                    if (value) {
                        const variable = `--b3-markdown-blockquote-marker-${property}`;
                        style.setProperty(variable, value);
                        this.variables.add(variable);
                    }
                });
            } catch {
                // 部分嵌入式浏览器不支持读取伪元素，块引用仍会使用主题边框色回退。
            }
        }
    }

    private scheduleRefresh() {
        this.window.cancelAnimationFrame(this.frame);
        this.frame = this.window.requestAnimationFrame(() => {
            this.frame = this.window.requestAnimationFrame(() => this.refresh());
        });
    }

    private createProbe() {
        const probe = this.document.createElement("div");
        probe.className = "protyle-wysiwyg markdown-theme-probe";
        probe.setAttribute("aria-hidden", "true");
        probe.setAttribute("inert", "");
        probe.innerHTML = `${[1, 2, 3, 4, 5, 6].map((level) => `<div class="h${level} markdown-theme-probe__h${level}" data-node-id="markdown-theme-h${level}" data-type="NodeHeading" data-subtype="h${level}"><div contenteditable="true" spellcheck="false">Heading</div></div>`).join("")}
<div class="p markdown-theme-probe__paragraph" data-node-id="markdown-theme-p" data-type="NodeParagraph"><div contenteditable="true" spellcheck="false"><span data-markdown-theme-probe="strong" data-type="strong">Strong</span><span data-markdown-theme-probe="emphasis" data-type="em">Emphasis</span><span data-markdown-theme-probe="strikethrough" data-type="s">Strike</span><span data-markdown-theme-probe="highlight" data-type="mark">Mark</span><span data-markdown-theme-probe="inline-code" data-type="code">Code</span><a data-markdown-theme-probe="link" data-type="a" href="#">Link</a></div></div>
<div class="bq markdown-theme-probe__blockquote" data-node-id="markdown-theme-bq" data-type="NodeBlockquote"><div class="p" data-node-id="markdown-theme-bq-p" data-type="NodeParagraph"><div contenteditable="true">Quote</div></div></div>
<div class="code-block markdown-theme-probe__code" data-node-id="markdown-theme-code" data-type="NodeCodeBlock"><div class="hljs" contenteditable="true">Code</div></div>
<div class="table markdown-theme-probe__table" data-node-id="markdown-theme-table" data-type="NodeTable"><table><thead><tr><th>Head</th></tr></thead><tbody><tr><td>Cell</td></tr></tbody></table></div>`;
        return probe;
    }
}

export const acquireMarkdownThemeBridge = (document: Document) => {
    let bridge = bridges.get(document);
    if (!bridge) {
        bridge = new MarkdownThemeBridge(document);
        bridges.set(document, bridge);
    }
    bridge.acquire();
    let released = false;
    return () => {
        if (!released) {
            released = true;
            bridge.release();
        }
    };
};

export const refreshMarkdownThemeBridge = (document: Document) => {
    bridges.get(document)?.refresh();
};
