import { readFile, readdir, stat } from "node:fs/promises";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const chineseHeading = "明窗净几，字字轻语。";
const chineseTitle = "轻语 QingYu｜开源 Markdown 编辑器";
const primaryDescription = "轻语是一款无需账号的开源 Markdown 编辑器，支持所见即所得、源码编辑、可切换笔记目录，以及 WebDAV / S3 当前笔记目录同步。";
const coreProductCopy = "打开一份 Markdown，文字便自然铺开。所见即所得与源码模式，写的是同一份文件；无需账号，也不把笔记困在云端。";
const manifestoLines = [
  "写作，不必先搭一套系统。",
  "所见即所得与源码，只是同一份 Markdown 的两面。",
  "文件夹盛放篇章，图片与链接保持原来的模样。",
  "同步可以抵达远方，但只跟随你当前选择的笔记目录。",
  "工具退后一步，文字便向前一步。"
];
const forbiddenChunkFamilies = [
  "milkdown",
  "codemirror",
  "tauri",
  "mermaid",
  "katex",
  "code-editor-vendor",
  "markdown-source-editor-vendor",
  "diagram-vendor",
  "math-vendor"
];

function compareNames(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function toRelativeAssetPath(distDirectory, absolutePath) {
  return relative(distDirectory, absolutePath).split(sep).join("/");
}

async function listAssets(distDirectory, directory = distDirectory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const sortedEntries = entries.sort((left, right) => compareNames(left.name, right.name));
  const assets = [];

  for (const entry of sortedEntries) {
    const absolutePath = join(directory, entry.name);
    if (entry.isDirectory()) {
      assets.push(...await listAssets(distDirectory, absolutePath));
    } else if (entry.isFile()) {
      const details = await stat(absolutePath);
      assets.push({
        path: toRelativeAssetPath(distDirectory, absolutePath),
        bytes: details.size
      });
    }
  }

  return assets;
}

function readAttribute(tag, attribute) {
  const escapedAttribute = attribute.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const pattern = new RegExp(`(?:\\s)${escapedAttribute}\\s*=\\s*(?:"([^"]*)"|'([^']*)')`, "iu");
  const match = tag.match(pattern);
  return match?.[1] ?? match?.[2] ?? null;
}

function hasBooleanAttribute(tag, attribute) {
  const escapedAttribute = attribute.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  return new RegExp(`(?:\\s)${escapedAttribute}(?:\\s|=|>)`, "iu").test(tag);
}

function findTag(tags, attribute, value) {
  return tags.find((tag) => readAttribute(tag, attribute)?.toLowerCase() === value.toLowerCase());
}

function assertBuild(condition, message) {
  if (!condition) throw new Error(message);
}

function assetReferencePath(reference) {
  if (!reference?.startsWith("/") || reference.startsWith("//")) return null;

  try {
    const url = new URL(reference, "https://build.invalid/");
    return decodeURIComponent(url.pathname).replace(/^\/+/, "");
  } catch {
    return null;
  }
}

function hasEmittedAsset(assets, reference) {
  const referencedPath = assetReferencePath(reference);
  return referencedPath !== null && assets.some(({ path }) => path === referencedPath);
}

function verifyMetadata(html, assets) {
  const linkTags = html.match(/<link\b[^>]*>/giu) ?? [];
  const metaTags = html.match(/<meta\b[^>]*>/giu) ?? [];
  const title = html.match(/<title\b[^>]*>([\s\S]*?)<\/title>/iu)?.[1].trim();
  assertBuild(title === chineseTitle, "Missing required Chinese HTML title.");

  const description = findTag(metaTags, "name", "description");
  assertBuild(
    description && readAttribute(description, "content") === primaryDescription,
    "Missing required primary description."
  );

  const favicon = findTag(linkTags, "rel", "icon");
  assertBuild(favicon && readAttribute(favicon, "href"), "Missing required favicon markup.");

  const themeColor = findTag(metaTags, "name", "theme-color");
  assertBuild(themeColor && readAttribute(themeColor, "content"), "Missing required theme color markup.");

  const canonical = findTag(linkTags, "rel", "canonical");
  assertBuild(
    canonical
      && readAttribute(canonical, "href") === "/"
      && hasBooleanAttribute(canonical, "data-site-origin"),
    "Missing relative, marked canonical metadata."
  );

  const openGraphRequirements = ["og:type", "og:title", "og:description", "og:image"];
  for (const property of openGraphRequirements) {
    const tag = findTag(metaTags, "property", property);
    assertBuild(tag && readAttribute(tag, "content"), `Missing required Open Graph metadata: ${property}.`);
  }

  const openGraphImage = findTag(metaTags, "property", "og:image");
  const openGraphImageReference = openGraphImage && readAttribute(openGraphImage, "content");
  assertBuild(
    hasEmittedAsset(assets, openGraphImageReference),
    `The og:image reference ${openGraphImageReference ?? "unknown"} does not resolve to an emitted asset.`
  );

  const openGraphUrl = findTag(metaTags, "property", "og:url");
  assertBuild(
    openGraphUrl
      && readAttribute(openGraphUrl, "content") === "/"
      && hasBooleanAttribute(openGraphUrl, "data-site-origin"),
    "Missing relative, marked og:url metadata."
  );

  const twitterRequirements = ["twitter:card", "twitter:title", "twitter:description", "twitter:image"];
  for (const name of twitterRequirements) {
    const tag = findTag(metaTags, "name", name);
    assertBuild(tag && readAttribute(tag, "content"), `Missing required Twitter Card metadata: ${name}.`);
  }

  const jsonLdScripts = [...html.matchAll(/<script\b[^>]*type=["']application\/ld\+json["'][^>]*>([\s\S]*?)<\/script>/giu)];
  const hasSoftwareApplication = jsonLdScripts.some((match) => {
    try {
      const data = JSON.parse(match[1]);
      return data["@type"] === "SoftwareApplication";
    } catch {
      return false;
    }
  });
  assertBuild(hasSoftwareApplication, "Missing valid SoftwareApplication JSON-LD markup.");

  const woff2Preloads = linkTags.filter((tag) => (
    readAttribute(tag, "rel")?.toLowerCase() === "preload"
      && readAttribute(tag, "href")?.toLowerCase().endsWith(".woff2")
  ));
  assertBuild(woff2Preloads.length === 1, "Expected exactly one WOFF2 preload.");
  const [woff2Preload] = woff2Preloads;
  assertBuild(
    readAttribute(woff2Preload, "as")?.toLowerCase() === "font"
      && readAttribute(woff2Preload, "type")?.toLowerCase() === "font/woff2"
      && hasBooleanAttribute(woff2Preload, "crossorigin"),
    "The WOFF2 preload must declare font type and crossorigin."
  );
  const woff2Reference = readAttribute(woff2Preload, "href");
  assertBuild(
    hasEmittedAsset(assets, woff2Reference),
    `The WOFF2 preload reference ${woff2Reference ?? "unknown"} does not resolve to an emitted asset.`
  );
}

function verifyContent(html) {
  const chineseH1 = new RegExp(`<h1\\b[^>]*>[^<]*${chineseHeading}[^<]*<\\/h1>`, "u");
  assertBuild(chineseH1.test(html), "Missing prerendered Chinese h1.");
  assertBuild(html.includes(coreProductCopy), "Missing core product copy.");

  for (const line of manifestoLines) {
    assertBuild(html.includes(line), `Missing product principle: ${line}`);
  }

  assertBuild(
    !/(?:apps\.apple\.com|itunes\.apple\.com|play\.google\.com)\//iu.test(html),
    "Found forbidden mobile store link."
  );
}

function verifyAssetBoundaries(assets) {
  const forbiddenFont = assets.find(({ path }) => /\.(?:ttf|otf|woff)$/iu.test(path));
  assertBuild(!forbiddenFont, `Found forbidden font format: ${forbiddenFont?.path ?? "unknown"}`);

  const woff2Assets = assets.filter(({ path }) => /\.woff2$/iu.test(path));
  assertBuild(woff2Assets.length === 1, "Expected exactly one WOFF2 file.");

  const forbiddenChunk = assets.find(({ path }) => {
    const name = basename(path).toLowerCase();
    return forbiddenChunkFamilies.some((family) => name.includes(family));
  });
  assertBuild(!forbiddenChunk, `Found forbidden chunk family: ${forbiddenChunk?.path ?? "unknown"}`);

  assertBuild(
    assets.some(({ path }) => path === "og-image.png"),
    "Missing required og-image.png asset."
  );
}

export async function verifySiteBuild(distDirectory) {
  const resolvedDirectory = resolve(distDirectory);
  const html = await readFile(join(resolvedDirectory, "index.html"), "utf8");
  const assets = await listAssets(resolvedDirectory);

  verifyContent(html);
  verifyMetadata(html, assets);
  verifyAssetBoundaries(assets);

  return { assets };
}

function printAssetInventory(assets) {
  const pathWidth = Math.max("Asset".length, ...assets.map((asset) => asset.path.length));
  console.log("Verified QingYu site build assets:");
  console.log(`${"Asset".padEnd(pathWidth)}  Bytes`);
  for (const asset of assets) {
    console.log(`${asset.path.padEnd(pathWidth)}  ${String(asset.bytes).padStart(8)}`);
  }
}

const scriptPath = fileURLToPath(import.meta.url);
const isMain = process.argv[1] && resolve(process.argv[1]) === scriptPath;

if (isMain) {
  const siteDirectory = resolve(dirname(scriptPath), "..");
  const result = await verifySiteBuild(resolve(siteDirectory, "dist"));
  printAssetInventory(result.assets);
}
