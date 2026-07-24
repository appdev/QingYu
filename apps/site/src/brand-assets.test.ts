import { resolve } from "node:path";
import sharp from "sharp";

const repositoryRoot = resolve(process.cwd(), "../..");
const expectedAssets = [
  ["assets/branding/app-icon/web-icon-64.png", 64, 64, "png"],
  ["assets/branding/app-icon/web-icon-256.png", 256, 256, "png"],
  ["apps/site/public/qingyu-logo.png", 64, 64, "png"],
  ["apps/site/public/qingyu-logo.webp", 256, 256, "webp"],
  ["apps/site/public/og-image.png", 1200, 630, "png"]
] as const;

describe("QingYu site brand assets", () => {
  it.each(expectedAssets)(
    "%s is emitted at %d × %d as %s",
    async (relativePath, width, height, format) => {
      const metadata = await sharp(resolve(repositoryRoot, relativePath)).metadata();

      expect(metadata.width).toBe(width);
      expect(metadata.height).toBe(height);
      expect(metadata.format).toBe(format);
    }
  );
});
