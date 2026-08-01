import {
  contrastRatio,
  ensureContrast,
  fitContrast,
  parseComputedRgb,
  type RgbColor
} from "./workspace-home-contrast";

describe("workspace home contrast", () => {
  it("parses modern and legacy computed RGB colors", () => {
    expect(parseComputedRgb("rgb(35 40 45)")).toEqual([35, 40, 45]);
    expect(parseComputedRgb("rgba(255, 255, 255, 1)")).toEqual([255, 255, 255]);
    expect(parseComputedRgb("color(srgb 0.2 0.4 0.6 / 1)")).toEqual([51, 102, 153]);
    expect(parseComputedRgb("rgba(255, 255, 255, 0.5)")).toBeNull();
    expect(parseComputedRgb("transparent")).toBeNull();
  });

  it("returns the WCAG contrast ratio", () => {
    expect(contrastRatio([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 4);
    expect(contrastRatio([35, 40, 45], [35, 40, 45])).toBe(1);
  });

  it.each([
    { background: [35, 40, 45] as RgbColor, preferred: [231, 233, 234] as RgbColor },
    { background: [255, 255, 255] as RgbColor, preferred: [38, 38, 38] as RgbColor }
  ])("fits restrained brand colors to the requested thresholds", ({ background, preferred }) => {
    const base = fitContrast(background, preferred, 1.35);
    const slice = fitContrast(background, preferred, 1.6);

    expect(contrastRatio(background, base)).toBeGreaterThanOrEqual(1.35);
    expect(contrastRatio(background, base)).toBeLessThan(1.47);
    expect(contrastRatio(background, slice)).toBeGreaterThanOrEqual(1.6);
    expect(contrastRatio(background, slice)).toBeLessThan(1.72);
  });

  it("preserves compliant functional colors and repairs only failing ones", () => {
    const background: RgbColor = [255, 255, 255];
    const readable: RgbColor = [38, 38, 38];
    const lowContrast: RgbColor = [190, 190, 190];

    expect(ensureContrast(background, readable, 4.5)).toEqual(readable);
    expect(contrastRatio(background, ensureContrast(background, lowContrast, 4.5)))
      .toBeGreaterThanOrEqual(4.5);
  });
});
