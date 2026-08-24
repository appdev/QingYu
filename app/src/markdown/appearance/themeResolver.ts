import {appearanceVariableName, listAppearanceContracts, type MarkdownAppearanceContract} from "./contracts";
import {
    APPEARANCE_FIXTURE_MARKDOWN,
    createNativeAppearanceFixture,
    createNativeHeadingContextFixtures,
} from "./fixture";
import {createStaticProtyleLute} from "../../protyle/render/setLute";

export interface MarkdownAppearanceSnapshot {
    revision: number;
    values: Readonly<Record<string, string>>;
}

export interface MarkdownAppearanceHandle {
    refresh(): void;
    release(): void;
}

type LuteWindow = Window & {
    Lute?: {
        New(): {Md2BlockDOM(markdown: string): string; SetProtyleWYSIWYG?(enabled: boolean): void};
    };
};

const createFallbackStaticLute = (window: LuteWindow | null) => {
    const lute = window?.Lute?.New();
    lute?.SetProtyleWYSIWYG?.(true);
    return lute;
};

const resolvers = new WeakMap<Document, MarkdownAppearanceResolver>();
const invalidValues = new Set(["", "initial", "revert", "revert-layer", "unset"]);

const isValidValue = (value: string) => !invalidValues.has(value.trim().toLowerCase());

const semanticFallback = (contract: MarkdownAppearanceContract, property: string) => {
    if (contract.reference.kind === "variable" && contract.reference.variable &&
        /color|background|accent/iu.test(property)) {
        return contract.reference.variable;
    }
    const matches = (name: string) => {
        if (property === "backgroundColor") {
            return !name.includes("on-background") &&
                /(?:background|surface|list-hover|lightest|warning)/u.test(name);
        }
        if (/^(?:border.*Color)$/u.test(property)) return name.includes("border") || name.includes("primary");
        if (property === "color") {
            return /(?:on-(?:background|surface)|-color$|primary$|error$|warning$)/u.test(name) &&
                !/(?:lightest$|lighter$)/u.test(name);
        }
        if (property === "accentColor") return name.includes("primary");
        if (property === "fontFamily") return name.includes("font-family");
        if (property === "fontSize") return name.includes("font-size");
        if (property === "boxShadow") return name.includes("shadow");
        return false;
    };
    return contract.fallbackVariables.find(matches);
};

const readVariable = (element: Element, name: string | undefined) => {
    if (!name) return "";
    const document = element.ownerDocument;
    const value = document.defaultView?.getComputedStyle(element)
        .getPropertyValue(name).trim() ?? "";
    if (isValidValue(value)) return value;
    if (element === document.documentElement) return "";
    const rootValue = document.defaultView?.getComputedStyle(document.documentElement)
        .getPropertyValue(name).trim() ?? "";
    return isValidValue(rootValue) ? rootValue : "";
};

const createProbe = (document: Document) => {
    const probe = document.createElement("div");
    probe.className = "protyle";
    probe.dataset.markdownAppearanceProbe = "true";
    probe.setAttribute("aria-hidden", "true");
    probe.setAttribute("inert", "");
    probe.style.cssText = "contain:layout style;left:-100000px;pointer-events:none;position:fixed;top:0;visibility:hidden;width:720px";
    let nativeRoot = createNativeAppearanceFixture(document, "");
    const lute = createStaticProtyleLute() ?? createFallbackStaticLute(document.defaultView as LuteWindow | null);
    if (lute) {
        try {
            nativeRoot = createNativeAppearanceFixture(document, lute.Md2BlockDOM(APPEARANCE_FIXTURE_MARKDOWN));
        } catch {
            // Lute 探针失败时保留空的原生容器，并让各契约使用语义回退。
        }
    }
    const references = document.createElement("div");
    references.innerHTML = "<div class=\"block__icons\"><button class=\"block__icon\"><svg></svg></button></div>";
    probe.append(nativeRoot);
    if (lute) probe.append(createNativeHeadingContextFixtures(document, lute));
    probe.append(references);
    return probe;
};

const probeReference = (
    document: Document,
    probe: HTMLElement,
    contract: MarkdownAppearanceContract,
    property: string,
) => {
    const selector = contract.propertyReferences?.[property]?.selector ?? contract.reference.selector;
    if (!contract.probe || !selector) return null;
    const reference = probe.matches(selector)
        ? probe
        : probe.querySelector<HTMLElement>(selector);
    const pseudo = contract.propertyReferences?.[property]?.pseudo;
    return reference ? document.defaultView?.getComputedStyle(reference, pseudo) ?? null : null;
};

const readProperty = (computed: CSSStyleDeclaration | null, property: string) => {
    if (!computed) return "";
    const value = computed[property as keyof CSSStyleDeclaration];
    return typeof value === "string" && isValidValue(value) ? value.trim() : "";
};

const decomposeInsetRing = (boxShadow: string) => {
    const colorFirst = /^(.+?)\s+0(?:px)?\s+0(?:px)?\s+0(?:px)?\s+([0-9]*\.?[0-9]+px)\s+inset$/u.exec(boxShadow);
    const insetFirst = /^inset\s+0(?:px)?\s+0(?:px)?\s+0(?:px)?\s+([0-9]*\.?[0-9]+px)\s+(.+)$/u.exec(boxShadow);
    const color = colorFirst?.[1] ?? insetFirst?.[2];
    const width = colorFirst?.[2] ?? insetFirst?.[1];
    if (!color || !width) return {first: "none", inline: "none", last: "none"};
    const inline = `${color} ${width} 0 0 inset, ${color} -${width} 0 0 inset`;
    return {
        first: `${inline}, ${color} 0 ${width} 0 inset`,
        inline,
        last: `${inline}, ${color} 0 -${width} 0 inset`,
    };
};

const resolveValues = (
    document: Document,
    probe: HTMLElement,
) => {
    const values: Record<string, string> = {};
    for (const contract of listAppearanceContracts()) {
        for (const property of contract.styleProperties) {
            const computed = probeReference(document, probe, contract, property);
            const computedProperty = contract.propertyReferences?.[property]?.property ?? property;
            const variable = appearanceVariableName(contract.id, property);
            const directVariable = contract.reference.kind === "variable"
                ? readVariable(probe, contract.reference.variable)
                : "";
            const probed = readProperty(computed, computedProperty);
            const fallback = readVariable(probe, semanticFallback(contract, property));
            const resolved = directVariable || probed || fallback;
            if (resolved) {
                values[variable] = resolved;
                if (property === "boxShadow" && contract.id.startsWith("block.callout-")) {
                    const edges = decomposeInsetRing(resolved);
                    values[appearanceVariableName(contract.id, "boxShadowFirst")] = edges.first;
                    values[appearanceVariableName(contract.id, "boxShadowInline")] = edges.inline;
                    values[appearanceVariableName(contract.id, "boxShadowLast")] = edges.last;
                }
            }
        }
    }
    return values;
};

const recordsEqual = (left: Readonly<Record<string, string>>, right: Readonly<Record<string, string>>) => {
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    return leftKeys.length === rightKeys.length && leftKeys.every((key) => left[key] === right[key]);
};

class MarkdownAppearanceResolver {
    private readonly document: Document;
    private readonly window: Window;
    private readonly probe: HTMLElement;
    private readonly observer: MutationObserver;
    private readonly roots = new Map<HTMLElement, number>();
    private snapshot: MarkdownAppearanceSnapshot = {revision: 0, values: Object.freeze({})};
    private readonly appliedVariables = new Map<HTMLElement, Set<string>>();
    private frame = 0;
    private disposed = false;
    private readonly onLoad = (event: Event) => {
        if (["LINK", "STYLE"].includes((event.target as Element | null)?.tagName ?? "")) {
            this.scheduleRefresh();
        }
    };

    constructor(document: Document) {
        const ownerWindow = document.defaultView;
        if (!ownerWindow) throw new Error("Markdown appearance resolver requires a browser document");
        this.document = document;
        this.window = ownerWindow;
        this.probe = createProbe(document);
        document.body.append(this.probe);
        this.observer = new ownerWindow.MutationObserver(() => this.scheduleRefresh());
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
    }

    public acquire(root: HTMLElement): MarkdownAppearanceHandle {
        if (this.disposed) throw new Error("Markdown appearance resolver has been released");
        this.roots.set(root, (this.roots.get(root) ?? 0) + 1);
        this.refresh();
        let released = false;
        return {
            refresh: () => this.refresh(),
            release: () => {
                if (released) return;
                released = true;
                this.release(root);
            },
        };
    }

    public refresh() {
        if (this.disposed) return;
        const values = resolveValues(this.document, this.probe);
        if (!recordsEqual(values, this.snapshot.values)) {
            this.snapshot = {
                revision: this.snapshot.revision + 1,
                values: Object.freeze(values),
            };
        }
        this.roots.forEach((_references, root) => this.apply(root));
    }

    private apply(root: HTMLElement) {
        const previous = this.appliedVariables.get(root) ?? new Set<string>();
        previous.forEach((name) => {
            if (!(name in this.snapshot.values)) root.style.removeProperty(name);
        });
        Object.entries(this.snapshot.values).forEach(([name, value]) => root.style.setProperty(name, value));
        this.appliedVariables.set(root, new Set(Object.keys(this.snapshot.values)));
        root.dataset.markdownAppearanceRevision = String(this.snapshot.revision);
    }

    private release(root: HTMLElement) {
        const references = this.roots.get(root) ?? 0;
        if (references > 1) {
            this.roots.set(root, references - 1);
            return;
        }
        this.roots.delete(root);
        (this.appliedVariables.get(root) ?? new Set(Object.keys(this.snapshot.values)))
            .forEach((name) => root.style.removeProperty(name));
        this.appliedVariables.delete(root);
        delete root.dataset.markdownAppearanceRevision;
        if (this.roots.size > 0) return;
        this.dispose();
    }

    private dispose() {
        this.disposed = true;
        this.observer.disconnect();
        this.document.removeEventListener("load", this.onLoad, true);
        this.window.cancelAnimationFrame(this.frame);
        this.probe.remove();
        resolvers.delete(this.document);
    }

    private scheduleRefresh() {
        this.window.cancelAnimationFrame(this.frame);
        this.frame = this.window.requestAnimationFrame(() => {
            this.frame = this.window.requestAnimationFrame(() => this.refresh());
        });
    }

    public static resolveOnce(document: Document): MarkdownAppearanceSnapshot {
        const probe = createProbe(document);
        document.body.append(probe);
        try {
            return {
                revision: 1,
                values: Object.freeze(resolveValues(document, probe)),
            };
        } finally {
            probe.remove();
        }
    }
}

export const acquireMarkdownAppearance = (root: HTMLElement): MarkdownAppearanceHandle => {
    const document = root.ownerDocument;
    let resolver = resolvers.get(document);
    if (!resolver) {
        resolver = new MarkdownAppearanceResolver(document);
        resolvers.set(document, resolver);
    }
    return resolver.acquire(root);
};

export const refreshMarkdownAppearance = (document: Document) => resolvers.get(document)?.refresh();

export const resolveMarkdownAppearanceForTest = (document: Document) =>
    MarkdownAppearanceResolver.resolveOnce(document);
