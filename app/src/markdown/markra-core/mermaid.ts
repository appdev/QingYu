type MarkraMermaidTheme = "base" | "dark" | "default" | "forest" | "neutral";
type MermaidRenderer = typeof import("mermaid")["default"];

type RenderMermaidOptions = {
  idPrefix?: string;
  theme?: MarkraMermaidTheme | string | null;
};

const darkMermaidThemeNames = new Set([
  "dark",
  "github-dark",
  "night",
  "one-dark",
  "one-dark-pro",
  "solarized-dark",
  "nord",
  "catppuccin-mocha"
]);

const darkNodeLabelColor = "#000000";
const lightNodeLabelColor = "#ffffff";
const minimumForegroundContrast = 4.5;
const svgNamespace = "http://www.w3.org/2000/svg";

type RgbColor = {
  alpha: number;
  blue: number;
  green: number;
  red: number;
};

let configuredTheme: MarkraMermaidTheme | null = null;
let mermaidRenderer: Promise<MermaidRenderer> | null = null;
let mermaidRenderSequence = 0;

export function isMermaidLanguage(language: string) {
  return language.toLowerCase() === "mermaid";
}

function isDarkMermaidThemeName(theme: string) {
  return darkMermaidThemeNames.has(theme.toLowerCase());
}

export function mermaidThemeFromElement(element: Element | null): MarkraMermaidTheme {
  const appearance = element?.ownerDocument.documentElement.getAttribute("data-theme-appearance");
  if (appearance === "dark") return "dark";
  if (appearance === "light") return "default";

  const theme = element?.closest(".markdown-paper")?.getAttribute("data-editor-theme") ??
    element?.ownerDocument.documentElement.getAttribute("data-theme") ??
    "";

  return isDarkMermaidThemeName(theme) ? "dark" : "default";
}

function normalizeMermaidTheme(theme: RenderMermaidOptions["theme"]): MarkraMermaidTheme {
  if (theme === "base" || theme === "dark" || theme === "forest" || theme === "neutral") return theme;
  return "default";
}

function loadMermaidRenderer() {
  mermaidRenderer ??= import("mermaid").then((module) => module.default);
  return mermaidRenderer;
}

function configureMermaid(renderer: MermaidRenderer, theme: MarkraMermaidTheme) {
  if (configuredTheme === theme) return;

  renderer.initialize({
    flowchart: {
      htmlLabels: true
    },
    securityLevel: "antiscript",
    startOnLoad: false,
    theme
  });
  configuredTheme = theme;
}

function parseHexColor(value: string): RgbColor | null {
  const match = /^#([\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/iu.exec(value);
  if (!match) return null;

  const hex = match[1];
  const expanded = hex.length <= 4
    ? Array.from(hex).map((character) => `${character}${character}`).join("")
    : hex;
  const hasAlpha = expanded.length === 8;

  return {
    alpha: hasAlpha ? Number.parseInt(expanded.slice(6, 8), 16) / 255 : 1,
    blue: Number.parseInt(expanded.slice(4, 6), 16),
    green: Number.parseInt(expanded.slice(2, 4), 16),
    red: Number.parseInt(expanded.slice(0, 2), 16)
  };
}

function parseRgbChannel(value: string) {
  const channel = Number.parseFloat(value);
  if (!Number.isFinite(channel)) return null;
  return value.endsWith("%") ? channel * 2.55 : channel;
}

function parseRgbColor(value: string): RgbColor | null {
  const normalized = value.trim().toLowerCase();
  if (normalized === "black") return { alpha: 1, blue: 0, green: 0, red: 0 };
  if (normalized === "white") return { alpha: 1, blue: 255, green: 255, red: 255 };
  if (normalized === "transparent") return null;

  const hex = parseHexColor(normalized);
  if (hex) return hex;

  const match = /^rgba?\((.+)\)$/u.exec(normalized);
  if (!match) return null;

  const parts = match[1]
    .replace(/\s*\/\s*/u, ",")
    .split(/[\s,]+/u)
    .filter(Boolean);
  if (parts.length !== 3 && parts.length !== 4) return null;

  const red = parseRgbChannel(parts[0]);
  const green = parseRgbChannel(parts[1]);
  const blue = parseRgbChannel(parts[2]);
  const parsedAlpha = parts[3] === undefined ? 1 : Number.parseFloat(parts[3]);
  const alpha = parts[3]?.endsWith("%") ? parsedAlpha / 100 : parsedAlpha;
  if (red === null || green === null || blue === null || !Number.isFinite(alpha)) return null;

  return {
    alpha: Math.min(1, Math.max(0, alpha)),
    blue: Math.min(255, Math.max(0, blue)),
    green: Math.min(255, Math.max(0, green)),
    red: Math.min(255, Math.max(0, red))
  };
}

function relativeLuminance(color: RgbColor) {
  const channelLuminance = (channel: number) => {
    const normalized = channel / 255;
    return normalized <= 0.04045
      ? normalized / 12.92
      : ((normalized + 0.055) / 1.055) ** 2.4;
  };

  return 0.2126 * channelLuminance(color.red) +
    0.7152 * channelLuminance(color.green) +
    0.0722 * channelLuminance(color.blue);
}

function contrastRatio(first: RgbColor, second: RgbColor) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  const lighter = Math.max(firstLuminance, secondLuminance);
  const darker = Math.min(firstLuminance, secondLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

function mixColor(source: RgbColor, target: RgbColor, amount: number): RgbColor {
  const mixChannel = (sourceChannel: number, targetChannel: number) =>
    sourceChannel + (targetChannel - sourceChannel) * amount;

  return {
    alpha: 1,
    blue: mixChannel(source.blue, target.blue),
    green: mixChannel(source.green, target.green),
    red: mixChannel(source.red, target.red)
  };
}

function rgbColorString(color: RgbColor) {
  return `rgb(${Math.round(color.red)}, ${Math.round(color.green)}, ${Math.round(color.blue)})`;
}

function renderedNodeFill(node: Element, getStyle: (element: Element) => CSSStyleDeclaration) {
  const shapes = node.querySelectorAll("rect, polygon, ellipse, circle, path");
  for (const shape of shapes) {
    if (shape.closest(".label") || shape.closest("foreignObject")) continue;
    const fill = parseRgbColor(getStyle(shape).fill);
    if (fill && fill.alpha >= 0.95) return fill;
  }
  return null;
}

function renderedLabelColor(
  label: HTMLElement | SVGElement,
  getStyle: (element: Element) => CSSStyleDeclaration
) {
  const style = getStyle(label);
  const candidates = label.namespaceURI === svgNamespace
    ? [style.fill, style.color]
    : [style.color, style.fill];

  for (const candidate of candidates) {
    const color = parseRgbColor(candidate);
    if (color && color.alpha >= 0.95) return color;
  }
  return null;
}

function renderedLabelTextElements(labels: readonly (HTMLElement | SVGElement)[]) {
  const textElements = new Set<HTMLElement | SVGElement>();

  for (const label of labels) {
    const candidates = [
      label,
      ...label.querySelectorAll<HTMLElement | SVGElement>("*")
    ];
    for (const candidate of candidates) {
      const hasDirectText = Array.from(candidate.childNodes).some((child) =>
        child.nodeType === 3 && Boolean(child.textContent?.trim())
      );
      if (hasDirectText) textElements.add(candidate);
    }
  }

  return textElements.size > 0 ? Array.from(textElements) : Array.from(labels);
}

function renderedSequenceForegroundColors(
  root: Element,
  getStyle: (element: Element) => CSSStyleDeclaration
) {
  const colors: RgbColor[] = [];
  const elements = root.querySelectorAll(
    ".messageText, .loopText, .sectionTitle, .actor-line, .messageLine0, .messageLine1, .loopLine"
  );

  for (const element of elements) {
    const style = getStyle(element);
    const value = element.matches("text, tspan") ? style.fill : style.stroke;
    const color = parseRgbColor(value);
    if (color && color.alpha >= 0.95) colors.push(color);
  }

  return colors;
}

function adjustedSequenceRectFill(
  background: RgbColor,
  foregrounds: readonly RgbColor[],
  primaryForeground: RgbColor,
  darkTarget: RgbColor,
  lightTarget: RgbColor
) {
  const target = relativeLuminance(primaryForeground) >= 0.5 ? darkTarget : lightTarget;
  for (let step = 1; step <= 100; step += 1) {
    const candidate = mixColor(background, target, step / 100);
    if (foregrounds.every((foreground) => contrastRatio(candidate, foreground) >= minimumForegroundContrast)) {
      return candidate;
    }
  }
  return target;
}

function ensureSequenceRectContrast(
  root: Element,
  getStyle: (element: Element) => CSSStyleDeclaration,
  darkTarget: RgbColor,
  lightTarget: RgbColor
) {
  const primaryElement = root.querySelector(".messageText, .loopText, .sectionTitle");
  if (!primaryElement) return 0;

  const primaryForeground = parseRgbColor(getStyle(primaryElement).fill);
  const foregrounds = renderedSequenceForegroundColors(root, getStyle);
  if (!primaryForeground || foregrounds.length === 0) return 0;

  let correctedRectCount = 0;
  for (const rect of root.querySelectorAll<SVGElement>("rect.rect")) {
    const background = parseRgbColor(getStyle(rect).fill);
    if (
      !background ||
      background.alpha < 0.95 ||
      contrastRatio(background, primaryForeground) >= minimumForegroundContrast
    ) {
      continue;
    }

    const replacement = adjustedSequenceRectFill(
      background,
      foregrounds,
      primaryForeground,
      darkTarget,
      lightTarget
    );
    rect.style.setProperty("fill", rgbColorString(replacement), "important");
    correctedRectCount += 1;
  }

  return correctedRectCount;
}

/** Correct low-contrast authored fills after Mermaid's SVG is mounted. */
export function ensureMermaidContrast(root: Element) {
  const view = root.ownerDocument.defaultView;
  if (!view) return 0;

  const getStyle = (element: Element) => view.getComputedStyle(element);
  const darkLabel = parseRgbColor(darkNodeLabelColor);
  const lightLabel = parseRgbColor(lightNodeLabelColor);
  if (!darkLabel || !lightLabel) return 0;

  let correctedNodeCount = 0;
  for (const node of root.querySelectorAll(".node")) {
    const background = renderedNodeFill(node, getStyle);
    const labels = Array.from(node.querySelectorAll<HTMLElement | SVGElement>(
      ".nodeLabel, .label text, .label tspan"
    ));
    const textElements = renderedLabelTextElements(labels);
    const currentColors = textElements
      .map((element) => renderedLabelColor(element, getStyle))
      .filter((color): color is RgbColor => color !== null);
    if (
      !background ||
      currentColors.length === 0 ||
      currentColors.every((color) => contrastRatio(background, color) >= minimumForegroundContrast)
    ) {
      continue;
    }

    const replacement = contrastRatio(background, darkLabel) >= contrastRatio(background, lightLabel)
      ? darkNodeLabelColor
      : lightNodeLabelColor;
    for (const element of new Set([...labels, ...textElements])) {
      element.style.setProperty("color", replacement, "important");
      if (element.namespaceURI === svgNamespace) {
        element.style.setProperty("fill", replacement, "important");
      }
    }
    correctedNodeCount += 1;
  }

  return correctedNodeCount + ensureSequenceRectContrast(
    root,
    getStyle,
    darkLabel,
    lightLabel
  );
}

export async function renderMermaidToSvg(source: string, options: RenderMermaidOptions = {}) {
  const definition = source.trim();
  if (!definition) return "";

  const theme = normalizeMermaidTheme(options.theme);
  const renderer = await loadMermaidRenderer();
  configureMermaid(renderer, theme);

  const idPrefix = options.idPrefix ?? "markra-mermaid";
  mermaidRenderSequence += 1;
  const result = await renderer.render(`${idPrefix}-${mermaidRenderSequence}`, definition);

  return result.svg;
}
