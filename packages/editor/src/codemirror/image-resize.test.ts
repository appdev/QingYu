import { describe, expect, it } from "vitest";
import {
  imageDragActivated,
  imageDragWidth,
  persistedImageWidth,
} from "./image-resize.ts";

describe("image resize geometry", () => {
  it("activates a drag only when either pointer axis reaches the threshold", () => {
    expect(imageDragActivated(4, 4)).toBe(false);
    expect(imageDragActivated(5, 0)).toBe(true);
  });

  it("rounds and clamps the dragged width to the available image range", () => {
    expect(imageDragWidth(100, -500, 800)).toBe(17);
    expect(imageDragWidth(100, 900, 800)).toBe(800);
    expect(imageDragWidth(100, 40, 800)).toBe(140);
    expect(imageDragWidth(100.4, 40.3, 800)).toBe(141);
  });

  it("does not persist a width within the default-width tolerance", () => {
    expect(persistedImageWidth(792, 800)).toBeNull();
    expect(persistedImageWidth(791, 800)).toBe(791);
  });
});
