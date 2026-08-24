import data = require("./contracts.json");

export type MarkdownAppearanceCategory = "native-equivalent" | "editor-foundation" | "markdown-exclusive";
export type MarkdownAppearanceMode = "source" | "visual";
export type MarkdownAppearancePlatform = "desktop" | "mobile";
export type MarkdownAppearanceState = "default" | "hover" | "focus" | "selected" | "disabled" |
    "editing" | "readonly" | "empty" | "error" | "expanded" | "drag" | "keyboard";
export type AppearanceGeometryMetric = "bottom" | "contentLeft" | "controlRight" | "height" | "left" |
    "top" | "width";
export type AppearanceGeometryBox = "border" | "margin";
export type AppearanceGeometryAggregate = "self" | "contiguous-lines";

export interface AppearanceGeometryReference {
    selector: string;
    closest?: string;
    box?: AppearanceGeometryBox;
    aggregate?: AppearanceGeometryAggregate;
}

export interface MarkdownAppearanceContract {
    id: string;
    category: MarkdownAppearanceCategory;
    markdownSelector: string;
    ownedSelectors: string[];
    reference: {
        kind: "native" | "component" | "variable";
        selector?: string;
        variable?: string;
    };
    propertyReferences?: Record<string, {selector: string; property: string; pseudo?: string}>;
    markdownPropertyReferences?: Record<string, {selector: string; property: string; pseudo?: string}>;
    geometryReferences?: {
        native: AppearanceGeometryReference;
        markdown: AppearanceGeometryReference;
    };
    states: MarkdownAppearanceState[];
    modes: MarkdownAppearanceMode[];
    platforms: MarkdownAppearancePlatform[];
    styleProperties: string[];
    comparisonProperties?: string[];
    geometry: AppearanceGeometryMetric[];
    fallbackVariables: string[];
    probe: boolean;
}

const categories = new Set<MarkdownAppearanceCategory>([
    "native-equivalent",
    "editor-foundation",
    "markdown-exclusive",
]);
const states = new Set<MarkdownAppearanceState>([
    "default",
    "hover",
    "focus",
    "selected",
    "disabled",
    "editing",
    "readonly",
    "empty",
    "error",
    "expanded",
    "drag",
    "keyboard",
]);
const modes = new Set<MarkdownAppearanceMode>(["source", "visual"]);
const platforms = new Set<MarkdownAppearancePlatform>(["desktop", "mobile"]);

const validateContracts = (value: unknown): readonly MarkdownAppearanceContract[] => {
    if (!Array.isArray(value)) {
        throw new TypeError("Markdown appearance contracts must be an array");
    }
    const ids = new Set<string>();
    const selectors = new Set<string>();
    return Object.freeze(value.map((candidate) => {
        const contract = candidate as MarkdownAppearanceContract;
        if (!contract.id || ids.has(contract.id)) {
            throw new TypeError(`Invalid Markdown appearance contract id: ${contract.id}`);
        }
        if (!categories.has(contract.category)) {
            throw new TypeError(`Invalid Markdown appearance category for ${contract.id}`);
        }
        if (!contract.states?.every((state) => states.has(state)) || contract.states.length === 0) {
            throw new TypeError(`Invalid Markdown appearance states for ${contract.id}`);
        }
        if (!contract.modes?.every((mode) => modes.has(mode)) || contract.modes.length === 0) {
            throw new TypeError(`Invalid Markdown appearance modes for ${contract.id}`);
        }
        if (!contract.platforms?.every((platform) => platforms.has(platform)) || contract.platforms.length === 0) {
            throw new TypeError(`Invalid Markdown appearance platforms for ${contract.id}`);
        }
        if (!contract.reference?.selector && !contract.reference?.variable) {
            throw new TypeError(`Missing Markdown appearance reference for ${contract.id}`);
        }
        if (contract.propertyReferences && Object.values(contract.propertyReferences).some((reference) =>
            !reference.selector || !reference.property || (reference.pseudo && !reference.pseudo.startsWith("::")))) {
            throw new TypeError(`Invalid Markdown appearance property reference for ${contract.id}`);
        }
        if (contract.markdownPropertyReferences && Object.values(contract.markdownPropertyReferences).some((reference) =>
            !reference.selector || !reference.property || (reference.pseudo && !reference.pseudo.startsWith("::")))) {
            throw new TypeError(`Invalid Markdown appearance Markdown property reference for ${contract.id}`);
        }
        if (contract.geometryReferences) {
            for (const [side, reference] of Object.entries(contract.geometryReferences)) {
                if (!reference.selector || (reference.box && !["border", "margin"].includes(reference.box)) ||
                    (reference.aggregate && !["self", "contiguous-lines"].includes(reference.aggregate))) {
                    throw new TypeError(`Invalid Markdown appearance ${side} geometry reference for ${contract.id}`);
                }
                if (side === "native" && reference.closest) {
                    throw new TypeError(`Native appearance geometry cannot use closest for ${contract.id}`);
                }
            }
        }
        if (!Array.isArray(contract.ownedSelectors)) {
            throw new TypeError(`Missing Markdown appearance selector ownership for ${contract.id}`);
        }
        for (const selector of [contract.markdownSelector, ...contract.ownedSelectors]) {
            if (!selector || selectors.has(selector)) {
                throw new TypeError(`Duplicate Markdown appearance selector owner: ${selector}`);
            }
            selectors.add(selector);
        }
        if (!contract.fallbackVariables?.every((name) => name.startsWith("--b3-"))) {
            throw new TypeError(`Invalid Markdown appearance fallback for ${contract.id}`);
        }
        if (contract.comparisonProperties?.some((property) => !contract.styleProperties.includes(property))) {
            throw new TypeError(`Invalid Markdown appearance comparison property for ${contract.id}`);
        }
        ids.add(contract.id);
        return Object.freeze(contract);
    }));
};

const contracts = validateContracts(data);

export const listAppearanceContracts = () => contracts;

export const getAppearanceContract = (id: string) => contracts.find((contract) => contract.id === id);

export const appearanceComparisonProperties = (contract: MarkdownAppearanceContract | undefined) => {
    if (!contract) return [];
    if (contract.comparisonProperties) return contract.comparisonProperties;
    return contract.category === "native-equivalent" ? contract.styleProperties : [];
};

export const appearanceVariableName = (id: string, property: string) =>
    `--b3-editor-appearance-${id.replace(/\./gu, "-")}-${property.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`)}`;
