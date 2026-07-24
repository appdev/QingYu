import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import { requireImageDimensions } from "./image-assets-lib.mjs";

const siteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(siteRoot, "../..");
const publicDirectory = resolve(siteRoot, "public");
const faviconIcon = resolve(repositoryRoot, "assets/branding/app-icon/web-icon-64.png");
const visibleIcon = resolve(repositoryRoot, "assets/branding/app-icon/web-icon-256.png");
const fontSourceArgument = process.argv.slice(2).find((argument) => argument.endsWith(".ttf"));
if (!fontSourceArgument?.endsWith(".ttf")) {
  throw new Error("Pass the verified LXGW WenKai GB Lite TTF path as the first argument.");
}
const fontSource = resolve(fontSourceArgument);

await mkdir(publicDirectory, { recursive: true });
await requireImageDimensions(faviconIcon, 64, 64);
await requireImageDimensions(visibleIcon, 256, 256);
await sharp(faviconIcon)
  .png({ compressionLevel: 9 })
  .toFile(resolve(publicDirectory, "qingyu-logo.png"));
await sharp(visibleIcon)
  .webp({ quality: 92, alphaQuality: 100, smartSubsample: true })
  .toFile(resolve(publicDirectory, "qingyu-logo.webp"));

const icon = await sharp(visibleIcon).resize(148, 148).png().toBuffer();
const title = await sharp({
  text: {
    text: '<span foreground="#181714">明窗净几，字字轻语。</span>',
    font: "LXGW WenKai GB Lite 58",
    fontfile: fontSource,
    rgba: true
  }
}).png().toBuffer();
const composition = Buffer.from(`
  <svg width="1200" height="630" xmlns="http://www.w3.org/2000/svg">
    <rect width="1200" height="630" fill="#f8f6f0"/>
    <rect x="64" y="56" width="1072" height="518" rx="34" fill="#fffdf7" stroke="#cfcbc1"/>
    <circle cx="138" cy="130" r="5" fill="#f9b52b"/>
    <text x="178" y="143" fill="#181714" font-family="sans-serif" font-size="38">QingYu</text>
    <text x="104" y="340" fill="#686158" font-family="sans-serif" font-size="25">A clear desk. An open file. Begin.</text>
    <rect x="700" y="112" width="356" height="360" rx="18" fill="#f3efe3" stroke="#cfcbc1"/>
    <rect x="700" y="112" width="92" height="360" rx="18" fill="#e8e2d3"/>
    <rect x="824" y="170" width="168" height="12" rx="6" fill="#3e3932"/>
    <rect x="824" y="218" width="192" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="244" width="164" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="270" width="180" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="330" width="116" height="8" rx="4" fill="#8e3328"/>
  </svg>
`);

await sharp(composition)
  .composite([
    { input: title, left: 104, top: 198 },
    { input: icon, left: 520, top: 403 }
  ])
  .png({ compressionLevel: 9 })
  .toFile(resolve(publicDirectory, "og-image.png"));
