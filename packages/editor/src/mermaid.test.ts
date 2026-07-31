import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("mermaid", () => ({
  default: {
    initialize: vi.fn(),
    render: vi.fn(async (id: string) => ({
      svg: `<svg id="${id}"><g></g></svg>`
    }))
  }
}));

import mermaid from "mermaid";
import { ensureMermaidContrast, renderMermaidToSvg } from "./mermaid";

function mountMermaidNode(fill: string, color: string, labelElement = "span") {
  const root = document.createElement("div");
  const label = labelElement === "span"
    ? `<foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel" style="color: ${color}">Label</span></div></foreignObject>`
    : `<${labelElement} class="nodeLabel" style="color: ${color}; fill: ${color}">Label</${labelElement}>`;
  root.innerHTML = [
    "<svg>",
    "  <g class=\"node\">",
    `    <rect style="fill: ${fill}"></rect>`,
    "    <g class=\"label\">",
    `      ${label}`,
    "    </g>",
    "  </g>",
    "</svg>"
  ].join("\n");
  document.body.append(root);
  return root;
}

function mountMermaidSequenceRect(fill: string, textColor: string, lineColor: string) {
  const root = document.createElement("div");
  root.innerHTML = [
    "<svg>",
    `  <rect class="rect" style="fill: ${fill}" x="10" y="20" width="200" height="120"></rect>`,
    `  <line class="actor-line" style="stroke: ${lineColor}" x1="30" y1="0" x2="30" y2="160"></line>`,
    `  <line class="messageLine0" style="stroke: ${textColor}" x1="30" y1="80" x2="180" y2="80"></line>`,
    `  <text class="messageText" style="fill: ${textColor}" x="100" y="70">Message</text>`,
    "</svg>"
  ].join("\n");
  document.body.append(root);
  return root;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("renderMermaidToSvg", () => {
  it("configures Mermaid to render safe HTML label line breaks", async () => {
    await renderMermaidToSvg(["flowchart TD", "  A[Global<br/>Rules] --> B[Project]"].join("\n"), {
      theme: "neutral"
    });

    expect(mermaid.initialize).toHaveBeenCalledWith(expect.objectContaining({
      flowchart: expect.objectContaining({
        htmlLabels: true
      }),
      securityLevel: "antiscript"
    }));
  });

  it("uses dark text when a light classDef fill makes a dark-theme label unreadable", () => {
    const root = mountMermaidNode("#e8f4fd", "#e0dfdf");
    const label = root.querySelector<HTMLElement>(".nodeLabel");

    expect(ensureMermaidContrast(root)).toBe(1);
    expect(label?.style.getPropertyValue("color")).toBe("rgb(0, 0, 0)");
    expect(label?.style.getPropertyPriority("color")).toBe("important");
  });

  it("uses light fill for an unreadable SVG text label on a dark node", () => {
    const root = mountMermaidNode("#181818", "#202124", "text");
    const label = root.querySelector<SVGElement>(".nodeLabel");

    expect(ensureMermaidContrast(root)).toBe(1);
    expect(label?.style.getPropertyValue("color")).toBe("rgb(255, 255, 255)");
    expect(label?.style.getPropertyValue("fill")).toBe("rgb(255, 255, 255)");
  });

  it("preserves an authored label color that already has sufficient contrast", () => {
    const root = mountMermaidNode("#e8f5e9", "#2e7d32");
    const label = root.querySelector<HTMLElement>(".nodeLabel");

    expect(ensureMermaidContrast(root)).toBe(0);
    expect(label?.style.getPropertyValue("color")).toBe("rgb(46, 125, 50)");
    expect(label?.style.getPropertyPriority("color")).toBe("");
  });

  it("preserves the dark theme's readable light text on dark nodes", () => {
    const root = mountMermaidNode("#181818", "#e0dfdf");
    const label = root.querySelector<HTMLElement>(".nodeLabel");

    expect(ensureMermaidContrast(root)).toBe(0);
    expect(label?.style.getPropertyValue("color")).toBe("rgb(224, 223, 223)");
    expect(label?.style.getPropertyPriority("color")).toBe("");
  });

  it("darkens light sequence rects when the dark theme foreground would be unreadable", () => {
    const root = mountMermaidSequenceRect("#e8f4fd", "#d3d3d3", "#cccccc");
    const rect = root.querySelector<SVGElement>("rect.rect");

    expect(ensureMermaidContrast(root)).toBe(1);
    const channels = rect?.style.getPropertyValue("fill").match(/\d+/gu)?.map(Number) ?? [];
    expect(channels).toHaveLength(3);
    expect(Math.max(...channels)).toBeLessThan(100);
    expect(rect?.style.getPropertyPriority("fill")).toBe("important");
  });

  it("preserves light sequence rects when their foreground is already readable", () => {
    const root = mountMermaidSequenceRect("#e8f4fd", "#333333", "#777777");
    const rect = root.querySelector<SVGElement>("rect.rect");

    expect(ensureMermaidContrast(root)).toBe(0);
    expect(rect?.style.getPropertyValue("fill")).toBe("rgb(232, 244, 253)");
    expect(rect?.style.getPropertyPriority("fill")).toBe("");
  });
});
