import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { renderSite } from "../dist-ssr/prerender.js";
import { injectPrerenderedRoot } from "./prerender-lib.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const siteDirectory = resolve(scriptDirectory, "..");
const indexPath = resolve(siteDirectory, "dist/index.html");
const temporaryPath = `${indexPath}.tmp-${process.pid}`;

const html = await readFile(indexPath, "utf8");
const prerenderedHtml = injectPrerenderedRoot(html, renderSite());

try {
  await writeFile(temporaryPath, prerenderedHtml, "utf8");
  await rename(temporaryPath, indexPath);
} finally {
  await rm(temporaryPath, { force: true });
}
