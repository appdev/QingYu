import {history} from "@codemirror/commands";
import {Compartment, EditorSelection, EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {openSearchPanel} from "@codemirror/search";
import type {App} from "../../index";
import {createStaticProtyleLute} from "../../protyle/render/setLute";
import type {MarkdownHostAdapter} from "../markra-core/adapter";
import {
    createClipboardUploadPlaceholder,
    createCodeMirrorBlockDropIndicator,
} from "../markra-core/codemirror";
import {createSiyuanMarkraExtension} from "../markraExtension";
import {MarkdownDocumentScrollController} from "../documentScroll";
import {reconfigureSiyuanMarkraExtension} from "../markdownEditorExtension";
import {listAppearanceContracts, type MarkdownAppearanceMode, type MarkdownAppearancePlatform} from "./contracts";
import {APPEARANCE_FIXTURE_MARKDOWN, createNativeAppearanceFixture} from "./fixture";
import {acquireMarkdownAppearance, type MarkdownAppearanceHandle} from "./themeResolver";

type AppearanceState = {
    mode?: number;
    themeDark?: string;
    themeLight?: string;
};

type LuteWindow = Window & {
    Lute?: {New(): {Md2BlockDOM(markdown: string): string; SetProtyleWYSIWYG?(enabled: boolean): void}};
};

const createFallbackStaticLute = (window: LuteWindow) => {
    const lute = window.Lute?.New();
    lute?.SetProtyleWYSIWYG?.(true);
    return lute;
};

export interface RuntimeHarnessOptions {
    markdown?: string;
    mode?: MarkdownAppearanceMode;
    platform?: MarkdownAppearancePlatform;
    width?: number;
}

export interface RuntimeHarnessDependencies {
    adapterFactory?(app: App): MarkdownHostAdapter | Promise<MarkdownHostAdapter>;
    loadThemeCss?(theme: "daylight" | "midnight"): Promise<string>;
    renderNative?(root: HTMLElement): void | Promise<void>;
}

export interface ApplicationAppearanceSnapshot {
    activeElement: Element | null;
    className: string;
    dataAttributes: Record<string, string>;
    mode: number | undefined;
    scrollX: number;
    scrollY: number;
    themeDark: string | undefined;
    themeLight: string | undefined;
}

export interface RuntimeAppearanceFixture {
    markdownRoot: HTMLElement;
    nativeRoot: HTMLElement;
    root: HTMLElement;
    view: EditorView;
    destroy(): Promise<void>;
}

export interface AppearanceMeasurement {
    contractId: string;
    geometryDiffs: Record<string, number>;
    markdown: {rect: Record<string, number>; styles: Record<string, string>} | null;
    native: {rect: Record<string, number>; styles: Record<string, string>} | null;
    state: string;
    styleDiffs: Record<string, {actual: string; expected: string}>;
    fallback?: string;
}

export interface RuntimeAppearanceReport {
    contractCount: number;
    maximumGeometryDifference: number;
    measurements: AppearanceMeasurement[];
    uncovered: string[];
}

export interface RuntimeDocumentBehaviorReport {
    documentScrollOwnerCount: number;
    documentScrollOwnerIsContent: boolean;
    renderedLineCount: number;
    titleLeavesViewport: boolean;
}

export interface RuntimeModeContinuityReport {
    anchorOffsetDifference: number;
    anchorPositionAfter: number;
    anchorPositionBefore: number;
    sameView: boolean;
}

export interface MarkdownAppearanceRuntimeHarness {
    destroy(): Promise<void>;
    interact(state: string): Promise<void>;
    measure(): RuntimeAppearanceReport;
    measureDocumentBehavior(): Promise<RuntimeDocumentBehaviorReport>;
    measureModeContinuity(): Promise<RuntimeModeContinuityReport>;
    mount(options?: RuntimeHarnessOptions): Promise<RuntimeAppearanceFixture>;
    setMode(mode: MarkdownAppearanceMode): Promise<void>;
    setTheme(theme: "daylight" | "midnight" | "standard-third-party"): Promise<void>;
}

const trackedDataAttributes = ["data-dark-theme", "data-light-theme", "data-theme-mode"];
const runtimeThemeScopes = [
    '[data-appearance-runtime="true"]',
    '.protyle[data-markdown-appearance-probe="true"]',
];

const scopeThemeCss = (css: string) => {
    const rules = css.replace(/\/\*[\s\S]*?\*\//gu, "").matchAll(/([^{}]+)\{([^{}]*)\}/gu);
    const scoped = Array.from(rules).flatMap(([, selectorText, declarations]) => {
        const selectors = selectorText.split(",").flatMap((selector) => {
            const normalized = selector.trim();
            return runtimeThemeScopes.map((scope) => normalized.startsWith(":root")
                ? `${scope}${normalized.slice(":root".length)}`
                : `${scope} ${normalized}`);
        });
        return [`${selectors.join(",")}{${declarations.trim()}}`];
    }).join("");
    if (!scoped.includes("--b3-theme-background")) {
        throw new Error("The Markdown appearance theme fixture is missing SiYuan root variables");
    }
    return scoped;
};

export const captureApplicationAppearance = (
    document: Document,
    appearance: AppearanceState,
): ApplicationAppearanceSnapshot => ({
    activeElement: document.activeElement,
    className: document.documentElement.className,
    dataAttributes: Object.fromEntries(trackedDataAttributes.flatMap((name) => {
        const value = document.documentElement.getAttribute(name);
        return value === null ? [] : [[name, value]];
    })),
    mode: appearance.mode,
    scrollX: document.defaultView?.scrollX ?? 0,
    scrollY: document.defaultView?.scrollY ?? 0,
    themeDark: appearance.themeDark,
    themeLight: appearance.themeLight,
});

const styleValue = (style: CSSStyleDeclaration, property: string) => {
    const value = style[property as keyof CSSStyleDeclaration];
    return typeof value === "string" ? value : "";
};

const serializeRect = (rect: DOMRect) => ({
    bottom: rect.bottom,
    height: rect.height,
    left: rect.left,
    right: rect.right,
    top: rect.top,
    width: rect.width,
});

const readElement = (
    element: HTMLElement,
    properties: readonly string[],
    rect = element.getBoundingClientRect(),
) => {
    const computed = element.ownerDocument.defaultView?.getComputedStyle(element);
    return {
        rect: serializeRect(rect),
        styles: Object.fromEntries(properties.map((property) => [
            property,
            computed ? styleValue(computed, property) : "",
        ])),
    };
};

const readPseudoElement = (
    element: HTMLElement,
    properties: readonly string[],
    pseudo: string,
) => {
    const computed = element.ownerDocument.defaultView?.getComputedStyle(element, pseudo);
    return {
        rect: serializeRect(element.getBoundingClientRect()),
        styles: Object.fromEntries(properties.map((property) => [
            property,
            computed ? styleValue(computed, property) : "",
        ])),
    };
};

const unionRect = (elements: readonly HTMLElement[]) => {
    const rects = elements.map((element) => element.getBoundingClientRect());
    const left = Math.min(...rects.map((rect) => rect.left));
    const right = Math.max(...rects.map((rect) => rect.right));
    const top = Math.min(...rects.map((rect) => rect.top));
    const bottom = Math.max(...rects.map((rect) => rect.bottom));
    return DOMRect.fromRect({height: bottom - top, width: right - left, x: left, y: top});
};

const collectContiguousLines = (element: HTMLElement, selector: string) => {
    const lines: HTMLElement[] = [];
    let line: Element | null = element.closest(".cm-line");
    while (line instanceof HTMLElement && line.matches(selector)) {
        lines.push(line);
        line = line.nextElementSibling;
    }
    return lines;
};

const markdownRect = (contractId: string, element: HTMLElement) => {
    if (contractId === "block.code") {
        const lines = collectContiguousLines(element, ".cm-markra-code-content-line");
        return lines.length > 0 ? unionRect(lines) : element.getBoundingClientRect();
    }
    if (contractId === "block.callout") {
        const lines = collectContiguousLines(element, ".cm-markra-callout");
        return lines.length > 0 ? unionRect(lines) : element.getBoundingClientRect();
    }
    return element.getBoundingClientRect();
};

const readNativeElement = (
    shell: HTMLElement,
    element: HTMLElement,
    contract: ReturnType<typeof listAppearanceContracts>[number],
) => {
    const result = readElement(element, contract.styleProperties);
    for (const property of contract.styleProperties) {
        const reference = contract.propertyReferences?.[property];
        if (!reference) continue;
        const propertyElement = shell.querySelector<HTMLElement>(reference.selector);
        const computed = propertyElement?.ownerDocument.defaultView?.getComputedStyle(propertyElement);
        result.styles[property] = computed ? styleValue(computed, reference.property ?? property) : "";
    }
    return result;
};

const geometryValue = (
    rect: Record<string, number>,
    container: Record<string, number>,
    metric: string,
) => {
    if (metric === "contentLeft" || metric === "left") return rect.left - container.left;
    if (metric === "controlRight") return container.right - rect.right;
    if (metric === "top") return rect.top - container.top;
    if (metric === "bottom") return rect.bottom - container.top;
    return rect[metric] ?? 0;
};

const createNativeShell = (document: Document, blockDOM: string, width: number) => {
    const shell = document.createElement("section");
    shell.className = "protyle markdown-appearance-runtime__native";
    shell.dataset.appearanceFixture = "native-shell";
    shell.style.height = "1400px";
    shell.style.width = `${width}px`;
    shell.innerHTML = "<div class=\"protyle-content\"><div class=\"protyle-background\"></div><div class=\"protyle-title\"><div class=\"protyle-title__input\">Appearance fixture</div></div></div>";
    const nativeRoot = createNativeAppearanceFixture(document, blockDOM);
    shell.querySelector(".protyle-content")?.append(nativeRoot);
    const references = document.createElement("div");
    references.className = "markdown-appearance-runtime__references";
    references.style.cssText = "contain:layout style;left:-100000px;position:fixed;top:0;visibility:hidden";
    references.innerHTML = "<div class=\"b3-typography\"></div><div class=\"b3-list\"></div><div class=\"b3-snackbar--error\"></div><div class=\"block__icons\"><button class=\"block__icon\"></button></div><div class=\"protyle-util\"></div><div class=\"b3-menu\"><input class=\"b3-text-field\"><button class=\"b3-list-item\"></button></div><div class=\"b3-progress\"></div><div class=\"viewer-container\"></div>";
    shell.append(references);
    return {nativeRoot, shell};
};

const createMarkdownShell = (
    document: Document,
    platform: MarkdownAppearancePlatform,
    width: number,
) => {
    const shell = document.createElement("section");
    shell.className = "protyle markdown-editor markdown-appearance-runtime__markdown";
    shell.dataset.markdownPlatform = platform;
    shell.style.height = "1400px";
    shell.style.width = `${width}px`;
    shell.innerHTML = "<div class=\"protyle-content markdown-editor__content\"><div class=\"protyle-top markdown-editor__top\"><div class=\"protyle-background markdown-editor__metadata\"></div><div class=\"protyle-title markdown-editor__title\"><div class=\"protyle-title__input\">Appearance fixture</div></div></div><div class=\"markdown-editor__body\" style=\"padding:16px 16px 0 24px\"><div class=\"markdown-editor__surface b3-typography\"></div></div></div><div class=\"markdown-editor__status\"></div>";
    return shell;
};

const waitForLayout = (window: Window) => new Promise<void>((resolve) => {
    let settled = false;
    const complete = () => {
        if (settled) return;
        settled = true;
        window.clearTimeout(fallback);
        resolve();
    };
    const fallback = window.setTimeout(complete, 250);
    window.requestAnimationFrame(() => window.requestAnimationFrame(complete));
});

const waitForMedia = async (root: HTMLElement) => {
    const images = [...root.querySelectorAll("img")].filter((image) =>
        Boolean(image.getAttribute("src") || image.dataset.src));
    images.forEach((image) => {
        if (!image.getAttribute("src") && image.dataset.src) image.src = image.dataset.src;
    });
    await Promise.all(images.map((image) => image.decode?.().catch(() => undefined)));
    await waitForLayout(root.ownerDocument.defaultView as Window);
};

export const installMarkdownAppearanceRuntimeHarness = (
    app: App,
    dependencies: RuntimeHarnessDependencies = {},
): MarkdownAppearanceRuntimeHarness => {
    let active: {
        adapter: MarkdownHostAdapter;
        appearanceHandle: MarkdownAppearanceHandle;
        documentScroll: MarkdownDocumentScrollController;
        fixture: RuntimeAppearanceFixture;
        mode: MarkdownAppearanceMode;
        modeCompartment: Compartment;
        nativeShell: HTMLElement;
        themeStyle: HTMLStyleElement | null;
    } | null = null;

    const createAdapter = async () => dependencies.adapterFactory
        ? dependencies.adapterFactory(app)
        : (await import("../siyuanAdapter")).createSiyuanMarkdownAdapter({
            app,
            documentPath: () => "/appearance-runtime.md",
        });

    const destroy = async () => {
        if (!active) return;
        const current = active;
        active = null;
        current.appearanceHandle.release();
        current.documentScroll.destroy();
        current.fixture.view.destroy();
        current.themeStyle?.remove();
        current.fixture.root.remove();
    };

    const mount = async (options: RuntimeHarnessOptions = {}): Promise<RuntimeAppearanceFixture> => {
        await destroy();
        const document = window.document;
        const markdown = options.markdown ?? APPEARANCE_FIXTURE_MARKDOWN;
        const mode = options.mode ?? "visual";
        const platform = options.platform ?? "desktop";
        const width = options.width ?? 500;
        const lute = createStaticProtyleLute() ?? createFallbackStaticLute(window as LuteWindow);
        if (!lute) throw new Error("Lute is required for the Markdown appearance runtime harness");
        const root = document.createElement("div");
        root.className = "markdown-appearance-runtime";
        root.dataset.appearanceRuntime = "true";
        root.style.cssText = "background:var(--b3-theme-background);color:var(--b3-theme-on-background);inset:0;overflow:auto;padding:16px;position:fixed;z-index:9999";
        const grid = document.createElement("div");
        grid.className = "markdown-appearance-runtime__grid";
        grid.style.cssText = "display:grid;gap:16px;grid-template-columns:repeat(2,minmax(0,1fr));min-width:0";
        const native = createNativeShell(document, lute.Md2BlockDOM(markdown), width);
        if (markdown.trim().length === 0) {
            native.nativeRoot.classList.add("protyle-wysiwyg--empty");
            native.nativeRoot.setAttribute("placeholder", window.siyuan?.languages?.emptyPlaceholder ?? "");
        }
        if (dependencies.renderNative) {
            await dependencies.renderNative(native.nativeRoot);
        } else {
            const [{processRender}, {highlightRender}] = await Promise.all([
                import("../../protyle/util/processCode"),
                import("../../protyle/render/highlightRender"),
            ]);
            processRender(native.nativeRoot);
            highlightRender(native.nativeRoot);
        }
        const markdownRoot = createMarkdownShell(document, platform, width);
        const surface = markdownRoot.querySelector<HTMLElement>(".markdown-editor__surface");
        const content = markdownRoot.querySelector<HTMLElement>(".markdown-editor__content");
        if (!surface || !content) throw new Error("Markdown appearance document shell was not created");
        grid.append(native.shell, markdownRoot);
        root.append(grid);
        document.body.append(root);

        const adapter = await createAdapter();
        const modeCompartment = new Compartment();
        const view = new EditorView({
            parent: surface,
            state: EditorState.create({
                doc: markdown,
                extensions: [
                    minimalSetup,
                    history(),
                    modeCompartment.of(createSiyuanMarkraExtension({
                        adapter,
                        documentPath: () => "/appearance-runtime.md",
                        getScrollContainer: () => content,
                        mode,
                    })),
                ],
            }),
        });
        const documentScroll = new MarkdownDocumentScrollController(() => view, content);
        const appearanceHandle = acquireMarkdownAppearance(markdownRoot);
        const fixture: RuntimeAppearanceFixture = {
            markdownRoot,
            nativeRoot: native.nativeRoot,
            root,
            view,
            destroy,
        };
        active = {
            adapter,
            appearanceHandle,
            documentScroll,
            fixture,
            mode,
            modeCompartment,
            nativeShell: native.shell,
            themeStyle: null,
        };
        await waitForLayout(window);
        await waitForMedia(root);
        return fixture;
    };

    const setTheme = async (theme: "daylight" | "midnight" | "standard-third-party") => {
        if (!active) throw new Error("Mount the Markdown appearance harness before setting a theme");
        active.themeStyle?.remove();
        const style = document.createElement("style");
        style.dataset.markdownAppearanceRuntimeTheme = theme;
        const sourceTheme = theme === "midnight" ? "midnight" : "daylight";
        const loadThemeCss = dependencies.loadThemeCss ?? (async (themeName: "daylight" | "midnight") => {
            const response = await window.fetch(`/appearance/themes/${themeName}/theme.css`);
            if (!response.ok) throw new Error(`Unable to load the SiYuan ${themeName} theme`);
            return response.text();
        });
        const themeCss = scopeThemeCss(await loadThemeCss(sourceTheme));
        const themeInheritance = `${runtimeThemeScopes.join(",")}{color:var(--b3-theme-on-background)}`;
        style.textContent = `${themeCss}${themeInheritance}${theme === "standard-third-party" ? '[data-appearance-runtime="true"] .protyle-wysiwyg .code-block,.protyle[data-markdown-appearance-probe="true"] .protyle-wysiwyg .code-block{border-radius:13px}' : ""}`;
        document.head.append(style);
        active.themeStyle = style;
        active.fixture.root.dataset.runtimeTheme = theme;
        active.appearanceHandle.refresh();
        await waitForLayout(window);
    };

    const setMode = async (mode: MarkdownAppearanceMode) => {
        if (!active || active.mode === mode) return;
        const content = active.fixture.markdownRoot.querySelector<HTMLElement>(".markdown-editor__content");
        reconfigureSiyuanMarkraExtension(active.fixture.view, active.modeCompartment, {
            adapter: active.adapter,
            documentPath: () => "/appearance-runtime.md",
            getScrollContainer: () => content,
            mode,
        }, active.documentScroll);
        active.mode = mode;
        await waitForLayout(window);
    };

    const interact = async (state: string) => {
        if (!active) throw new Error("Mount the Markdown appearance harness before interacting");
        const root = active.fixture.root;
        if (state === "focus") active.fixture.view.focus();
        if (state === "selected") {
            active.fixture.view.dispatch({
                selection: EditorSelection.range(0, Math.min(8, active.fixture.view.state.doc.length)),
            });
            active.fixture.view.focus();
        }
        if (state === "drag") {
            root.querySelector(".markra-block-drop-indicator")?.remove();
            const indicator = createCodeMirrorBlockDropIndicator(document);
            const line = active.fixture.view.dom.querySelector<HTMLElement>(".cm-line");
            const rect = line?.getBoundingClientRect();
            indicator.dataset.show = "true";
            indicator.style.left = `${rect?.left ?? 0}px`;
            indicator.style.top = `${rect?.bottom ?? 0}px`;
            indicator.style.width = `${rect?.width ?? 0}px`;
            active.fixture.view.dom.append(indicator);
        }
        if (state === "clipboard") {
            root.querySelector(".markra-image-upload-placeholder")?.remove();
            active.fixture.markdownRoot.append(createClipboardUploadPlaceholder(
                document,
                "appearance-runtime",
                window.siyuan?.languages?.uploading ?? "",
            ));
        }
        if (state === "error") {
            const status = root.querySelector<HTMLElement>(".markdown-editor__status");
            if (status) {
                status.dataset.status = "error";
                status.textContent = window.siyuan?.languages?.uploadError ?? "";
            }
        }
        if (state === "expanded") {
            root.querySelector<HTMLElement>(".markra-code-language-control")?.click();
            root.querySelector<HTMLElement>(".cm-markra-footnote-reference")
                ?.dispatchEvent(new MouseEvent("mouseenter", {bubbles: true}));
            openSearchPanel(active.fixture.view);
        }
        if (state === "media") {
            root.querySelector<HTMLElement>(".markra-image-node img")
                ?.dispatchEvent(new MouseEvent("dblclick", {bubbles: true}));
        }
        await waitForLayout(window);
    };

    const measure = (): RuntimeAppearanceReport => {
        if (!active) throw new Error("Mount the Markdown appearance harness before measuring");
        const current = active;
        const measurements = listAppearanceContracts()
            .filter((contract) => contract.modes.includes(current.mode))
            .map<AppearanceMeasurement>((contract) => {
                const mountedElement = current.fixture.root.querySelector<HTMLElement>(contract.markdownSelector);
                const overlayElement = contract.id.startsWith("overlay.")
                    ? document.querySelector<HTMLElement>(contract.markdownSelector)
                    : null;
                const selectionElement = contract.id === "editor.selection" &&
                    !current.fixture.view.state.selection.main.empty
                    ? current.fixture.view.contentDOM.querySelector<HTMLElement>(".cm-line") ??
                        current.fixture.view.contentDOM
                    : null;
                const markdownElement = mountedElement ?? overlayElement ?? selectionElement;
                const nativeElement = contract.reference.selector
                    ? current.nativeShell.querySelector<HTMLElement>(contract.reference.selector)
                    : null;
                const markdown = selectionElement && markdownElement === selectionElement
                    ? readPseudoElement(selectionElement, contract.styleProperties, "::selection")
                    : markdownElement
                        ? readElement(markdownElement, contract.styleProperties, markdownRect(contract.id, markdownElement))
                    : null;
                const native = nativeElement ? readNativeElement(current.nativeShell, nativeElement, contract) : null;
                const styleDiffs: AppearanceMeasurement["styleDiffs"] = {};
                const geometryDiffs: AppearanceMeasurement["geometryDiffs"] = {};
                if (markdown && native) {
                    const markdownContainer = serializeRect(current.fixture.markdownRoot.getBoundingClientRect());
                    const nativeContainer = serializeRect(current.nativeShell.getBoundingClientRect());
                    contract.styleProperties.forEach((property) => {
                        if (native.styles[property] !== markdown.styles[property]) {
                            styleDiffs[property] = {
                                actual: markdown.styles[property] ?? "",
                                expected: native.styles[property] ?? "",
                            };
                        }
                    });
                    if (contract.reference.kind === "native") {
                        contract.geometry.forEach((metric) => {
                            geometryDiffs[metric] = Math.abs(
                                geometryValue(markdown.rect, markdownContainer, metric) -
                                geometryValue(native.rect, nativeContainer, metric),
                            );
                        });
                    }
                }
                return {
                    contractId: contract.id,
                    fallback: markdown ? undefined : "not-mounted-in-current-state",
                    geometryDiffs,
                    markdown,
                    native,
                    state: selectionElement && markdownElement === selectionElement
                        ? "selected"
                        : markdownElement?.dataset.appearanceState ?? "default",
                    styleDiffs,
                };
            });
        const nativeEquivalentIds = new Set(listAppearanceContracts()
            .filter((contract) => contract.category === "native-equivalent")
            .map((contract) => contract.id));
        return {
            contractCount: listAppearanceContracts().length,
            maximumGeometryDifference: Math.max(
                0,
                ...measurements
                    .filter((item) => nativeEquivalentIds.has(item.contractId))
                    .flatMap((item) => Object.entries(item.geometryDiffs)
                        .filter(([metric]) => metric !== "top" && metric !== "bottom")
                        .map(([, value]) => value)),
            ),
            measurements,
            uncovered: measurements.filter((item) => !item.markdown).map((item) => item.contractId),
        };
    };

    const measureDocumentBehavior = async (): Promise<RuntimeDocumentBehaviorReport> => {
        if (!active) throw new Error("Mount the Markdown appearance harness before measuring document behavior");
        const content = active.fixture.markdownRoot.querySelector<HTMLElement>(".markdown-editor__content");
        const codeMirrorScroller = active.fixture.markdownRoot.querySelector<HTMLElement>(".cm-scroller");
        const title = active.fixture.markdownRoot.querySelector<HTMLElement>(".markdown-editor__title");
        if (!content || !codeMirrorScroller || !title) {
            throw new Error("The Markdown document scroll structure is incomplete");
        }
        const verticalOwners = [content, codeMirrorScroller].filter((element) => {
            const overflowY = window.getComputedStyle(element).overflowY;
            return /(auto|scroll)/u.test(overflowY) && element.scrollHeight > element.clientHeight;
        });
        content.scrollTop = Math.max(0, content.scrollHeight - content.clientHeight);
        await waitForLayout(window);
        const contentRect = content.getBoundingClientRect();
        const titleLeavesViewport = title.getBoundingClientRect().bottom < contentRect.top;
        const report = {
            documentScrollOwnerCount: verticalOwners.length,
            documentScrollOwnerIsContent: verticalOwners[0] === content,
            renderedLineCount: active.fixture.view.contentDOM.querySelectorAll(".cm-line").length,
            titleLeavesViewport,
        };
        content.scrollTop = 0;
        await waitForLayout(window);
        return report;
    };

    const measureModeContinuity = async (): Promise<RuntimeModeContinuityReport> => {
        if (!active) throw new Error("Mount the Markdown appearance harness before measuring mode continuity");
        const current = active;
        const content = current.fixture.markdownRoot.querySelector<HTMLElement>(".markdown-editor__content");
        if (!content) throw new Error("The Markdown document scroll container is missing");
        content.scrollTop = Math.max(0, (content.scrollHeight - content.clientHeight) / 2);
        await waitForLayout(window);
        const contentRect = content.getBoundingClientRect();
        const position = current.fixture.view.posAtCoords({
            x: contentRect.left + contentRect.width / 2,
            y: contentRect.top + contentRect.height / 2,
        }, false);
        if (position === null) throw new Error("CodeMirror did not resolve the viewport-center document position");
        current.fixture.view.dispatch({selection: {anchor: position}});
        const before = current.documentScroll.captureAnchor();
        if (!before) throw new Error("The Markdown mode-switch anchor could not be captured");
        const view = current.fixture.view;
        const originalMode = current.mode;
        const originalSelection = view.state.selection;
        await setMode(originalMode === "visual" ? "source" : "visual");
        const after = current.documentScroll.captureAnchor();
        if (!after) throw new Error("The Markdown mode-switch anchor could not be restored");
        const report = {
            anchorOffsetDifference: Math.abs(after.viewportOffset - before.viewportOffset),
            anchorPositionAfter: after.position,
            anchorPositionBefore: before.position,
            sameView: current.fixture.view === view,
        };
        await setMode(originalMode);
        view.dispatch({selection: originalSelection});
        content.scrollTop = 0;
        await waitForLayout(window);
        return report;
    };

    const api = {
        destroy,
        interact,
        measure,
        measureDocumentBehavior,
        measureModeContinuity,
        mount,
        setMode,
        setTheme,
    };
    window.__siyuanMarkdownAppearanceTest = api;
    return api;
};
