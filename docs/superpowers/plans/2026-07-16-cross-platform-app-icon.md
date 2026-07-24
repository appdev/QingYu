# Cross-Platform App Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every QingYu brand icon with the supplied artwork, reusing QingYu's already processed macOS ICNS and providing native assets for Windows, Linux, Android, iOS, README, and web surfaces.

**Architecture:** Keep `logo.png` as the canonical flattened source, use a deterministic Sharp-based generator for Tauri's standard platform sets, and import QingYu's processed `build/icon.icns` as the canonical macOS asset. The generator restores the imported ICNS after Tauri runs so future generation remains deterministic without depending on the sibling checkout.

**Tech Stack:** pnpm 10.30.3, Node.js, Sharp, Tauri CLI 2.11.0, Vitest, macOS `iconutil`, and `plutil`.

## Global Constraints

- Use `pnpm` for every JavaScript dependency and script workflow.
- Keep `pnpm-lock.yaml`; do not add another package-manager lockfile.
- Keep `logo.png` as the canonical flattened brand artwork.
- Preserve the yellow background treatment and white feather silhouette; do not regenerate the brand mark.
- macOS 26 and older macOS releases must use the processed ICNS imported from `/Volumes/extendData/Data/IdeaProjects/QingYu/build/icon.icns`.
- The imported ICNS must remain reproducible after the external QingYu checkout is no longer available.
- Windows ICO must contain 16, 24, 32, 48, 64, and 256 pixel layers.
- Linux PNG output must be square 32-bit RGBA.
- iOS output must be opaque and full-bleed, without a baked rounded enclosure; the platform applies its own mask.
- Android launcher foreground must remain inside the adaptive-icon safe zone.
- Preserve unrelated user files, including the untracked `bg.png`.

---

### Task 1: Deterministic Layer And Standard Icon Generator

**Files:**
- Create: `packages/scripts/src/app-icons/generate.mjs`
- Create: `packages/scripts/src/app-icons/generate.test.mjs`
- Modify: `packages/scripts/package.json`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Create: `assets/branding/app-icon/background.png` (generated)
- Create: `assets/branding/app-icon/feather.png` (generated)
- Create: `assets/branding/app-icon/platform-master.png` (generated)
- Replace: `apps/desktop/src-tauri/icons/**/*` (generated)
- Create: `apps/desktop/public/favicon.png` (generated)
- Create: `apps/web/public/favicon.png` (generated)

**Interfaces:**
- Consumes: root `logo.png`; `sharp` image operations; `pnpm --filter @markra/desktop tauri icon`.
- Produces: `buildIconLayers(inputPath: string, outputDirectory: string): Promise<{ backgroundPath: string; featherPath: string; platformMasterPath: string }>` and complete Tauri/Web icon assets.

- [ ] **Step 1: Add a failing synthetic-image test for layer separation**

```js
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import sharp from "sharp";
import { buildIconLayers } from "./generate.mjs";

describe("buildIconLayers", () => {
  test("removes edge-connected white pixels but preserves an enclosed white mark", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icon-"));
    const inputPath = join(directory, "source.png");
    const outputDirectory = join(directory, "output");

    await sharp({
      create: { width: 32, height: 32, channels: 4, background: "#ffffff" }
    })
      .composite([
        { input: Buffer.from('<svg width="24" height="24"><rect width="24" height="24" rx="5" fill="#f9b52b"/><path d="M12 5L16 16L12 13L8 19L10 10Z" fill="white"/></svg>'), left: 4, top: 4 }
      ])
      .png()
      .toFile(inputPath);

    const paths = await buildIconLayers(inputPath, outputDirectory, 32);
    const master = await sharp(await readFile(paths.platformMasterPath)).ensureAlpha().raw().toBuffer();
    const background = await sharp(await readFile(paths.backgroundPath)).ensureAlpha().raw().toBuffer();
    const feather = await sharp(await readFile(paths.featherPath)).ensureAlpha().raw().toBuffer();
    const alpha = (buffer, x, y) => buffer[(y * 32 + x) * 4 + 3];

    expect(alpha(master, 0, 0)).toBe(0);
    expect(alpha(master, 16, 16)).toBe(255);
    expect(alpha(background, 16, 12)).toBe(255);
    expect(alpha(feather, 16, 12)).toBeGreaterThan(200);
    expect(alpha(feather, 6, 6)).toBe(0);
  });
});
```

- [ ] **Step 2: Run the focused test and confirm the missing module failure**

Run: `pnpm --filter @markra/scripts test -- src/app-icons/generate.test.mjs`

Expected: FAIL because `packages/scripts/src/app-icons/generate.mjs` and `sharp` do not exist.

- [ ] **Step 3: Add Sharp and implement deterministic mask/fill generation**

Run: `pnpm --filter @markra/scripts add sharp`

Implement `generate.mjs` with these exported operations:

```js
import { mkdir, copyFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const currentDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(currentDirectory, "../../../..");

function isEdgeWhite(r, g, b) {
  return r >= 238 && g >= 238 && b >= 238 && Math.max(r, g, b) - Math.min(r, g, b) <= 18;
}

function isFeatherWhite(r, g, b) {
  return r >= 218 && g >= 218 && b >= 218 && Math.max(r, g, b) - Math.min(r, g, b) <= 28;
}

export function edgeConnectedWhiteMask(data, width, height) {
  const mask = new Uint8Array(width * height);
  const queue = new Uint32Array(width * height);
  let head = 0;
  let tail = 0;
  const enqueue = (index) => {
    if (mask[index]) return;
    const offset = index * 4;
    if (!isEdgeWhite(data[offset], data[offset + 1], data[offset + 2])) return;
    mask[index] = 1;
    queue[tail++] = index;
  };

  for (let x = 0; x < width; x += 1) {
    enqueue(x);
    enqueue((height - 1) * width + x);
  }
  for (let y = 0; y < height; y += 1) {
    enqueue(y * width);
    enqueue(y * width + width - 1);
  }
  while (head < tail) {
    const index = queue[head++];
    const x = index % width;
    const y = Math.floor(index / width);
    if (x > 0) enqueue(index - 1);
    if (x + 1 < width) enqueue(index + 1);
    if (y > 0) enqueue(index - width);
    if (y + 1 < height) enqueue(index + width);
  }
  return mask;
}

function fillFeatherAreaFromNearestPlate(source, externalMask, featherMask, width, height) {
  const output = Buffer.from(source);
  const distance = new Int32Array(width * height).fill(-1);
  const queue = new Uint32Array(width * height);
  let head = 0;
  let tail = 0;
  for (let index = 0; index < width * height; index += 1) {
    if (!externalMask[index] && !featherMask[index]) {
      distance[index] = 0;
      queue[tail++] = index;
    }
  }
  const visit = (from, next) => {
    if (distance[next] !== -1 || externalMask[next]) return;
    distance[next] = distance[from] + 1;
    const sourceOffset = from * 4;
    const nextOffset = next * 4;
    output[nextOffset] = output[sourceOffset];
    output[nextOffset + 1] = output[sourceOffset + 1];
    output[nextOffset + 2] = output[sourceOffset + 2];
    output[nextOffset + 3] = 255;
    queue[tail++] = next;
  };
  while (head < tail) {
    const index = queue[head++];
    const x = index % width;
    const y = Math.floor(index / width);
    if (x > 0) visit(index, index - 1);
    if (x + 1 < width) visit(index, index + 1);
    if (y > 0) visit(index, index - width);
    if (y + 1 < height) visit(index, index + width);
  }
  return output;
}

export async function buildIconLayers(inputPath, outputDirectory, size = 1024) {
  const { data, info } = await sharp(inputPath).resize(size, size, { fit: "fill" }).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const externalMask = edgeConnectedWhiteMask(data, info.width, info.height);
  const featherMask = new Uint8Array(info.width * info.height);
  const master = Buffer.from(data);
  const feather = Buffer.alloc(data.length);

  for (let index = 0; index < info.width * info.height; index += 1) {
    const offset = index * 4;
    if (externalMask[index]) master[offset + 3] = 0;
    if (!externalMask[index] && isFeatherWhite(data[offset], data[offset + 1], data[offset + 2])) {
      featherMask[index] = 1;
      feather[offset] = data[offset];
      feather[offset + 1] = data[offset + 1];
      feather[offset + 2] = data[offset + 2];
      feather[offset + 3] = 255;
    }
  }

  const background = fillFeatherAreaFromNearestPlate(master, externalMask, featherMask, info.width, info.height);
  await mkdir(outputDirectory, { recursive: true });
  const paths = {
    backgroundPath: join(outputDirectory, "background.png"),
    featherPath: join(outputDirectory, "feather.png"),
    platformMasterPath: join(outputDirectory, "platform-master.png")
  };
  await sharp(background, { raw: info }).png().toFile(paths.backgroundPath);
  await sharp(feather, { raw: info }).png().toFile(paths.featherPath);
  await sharp(master, { raw: info }).png().toFile(paths.platformMasterPath);
  return paths;
}

async function main() {
  const source = join(repoRoot, "logo.png");
  const brandingDirectory = join(repoRoot, "assets/branding/app-icon");
  const paths = await buildIconLayers(source, brandingDirectory);
  const result = spawnSync("pnpm", ["--filter", "@markra/desktop", "tauri", "icon", paths.platformMasterPath, "--ios-color", "#F9B52B"], { cwd: repoRoot, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  await mkdir(join(repoRoot, "apps/desktop/public"), { recursive: true });
  await mkdir(join(repoRoot, "apps/web/public"), { recursive: true });
  await copyFile(join(repoRoot, "apps/desktop/src-tauri/icons/32x32.png"), join(repoRoot, "apps/desktop/public/favicon.png"));
  await copyFile(join(repoRoot, "apps/desktop/src-tauri/icons/32x32.png"), join(repoRoot, "apps/web/public/favicon.png"));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch((error) => { console.error(error); process.exit(1); });
```

Add the root script:

```json
"icons:generate": "node packages/scripts/src/app-icons/generate.mjs"
```

- [ ] **Step 4: Run the focused test and generation command**

Run:

```bash
pnpm --filter @markra/scripts test -- src/app-icons/generate.test.mjs
pnpm icons:generate
```

Expected: the focused test passes; Tauri reports generated icons; `file` reports RGBA for desktop PNG files and RGB for iOS PNG files; iOS outputs match the generated full-bleed `ios-master.png`.

- [ ] **Step 5: Visually inspect the four 1024px source outputs and smallest desktop icon**

Open `background.png`, `feather.png`, `platform-master.png`, `ios-master.png`, and `32x32.png`. Confirm the outer white canvas is transparent on desktop outputs, the feather is not cropped, the background has no white feather-shaped hole, and the iOS master is an opaque full-bleed square without a rounded enclosure.

- [ ] **Step 6: Commit the deterministic generator and standard platform assets**

```bash
git add package.json pnpm-lock.yaml packages/scripts/package.json packages/scripts/src/app-icons assets/branding/app-icon apps/desktop/src-tauri/icons apps/desktop/public/favicon.png apps/web/public/favicon.png logo.png
git commit -m "feat: generate platform-adapted app icons"
```

### Task 2: Import The Processed QingYu macOS Icon

**Files:**
- Create: `assets/branding/app-icon/macos-icon.icns`
- Modify: `packages/scripts/src/app-icons/generate.mjs`
- Modify: `packages/scripts/src/app-icons/generate.test.mjs`
- Replace: `apps/desktop/src-tauri/icons/icon.icns`

**Interfaces:**
- Consumes once: `/Volumes/extendData/Data/IdeaProjects/QingYu/build/icon.icns`.
- Produces: a committed canonical `macos-icon.icns` and deterministic post-generation restoration to `apps/desktop/src-tauri/icons/icon.icns`.

- [ ] **Step 1: Verify the external source and built-app provenance**

```bash
shasum -a 256 /Volumes/extendData/Data/IdeaProjects/QingYu/logo.png logo.png
cmp -s /Volumes/extendData/Data/IdeaProjects/QingYu/build/icon.icns /Volumes/extendData/Data/IdeaProjects/QingYu/dist/mac-arm64/QingYu.app/Contents/Resources/icon.icns
plutil -p /Volumes/extendData/Data/IdeaProjects/QingYu/dist/mac-arm64/QingYu.app/Contents/Info.plist | rg 'CFBundleIconFile|icon.icns'
```

Expected: the two logo hashes are identical, `cmp` exits 0, and the built QingYu app declares `CFBundleIconFile` as `icon.icns`.

- [ ] **Step 2: Add a failing generator test for canonical ICNS restoration**

```js
test("restoreCanonicalMacIcon copies the imported processed ICNS", async () => {
  const directory = await mkdtemp(join(tmpdir(), "markra-mac-icon-"));
  const sourcePath = join(directory, "source.icns");
  const outputPath = join(directory, "output.icns");
  await writeFile(sourcePath, Buffer.from("processed-qingyu-icon"));
  await restoreCanonicalMacIcon(sourcePath, outputPath);
  expect(await readFile(outputPath)).toEqual(Buffer.from("processed-qingyu-icon"));
});
```

- [ ] **Step 3: Run the focused test and confirm the helper is missing**

Run: `pnpm --filter @markra/scripts test -- src/app-icons/generate.test.mjs`

Expected: FAIL because `restoreCanonicalMacIcon` is not exported.

- [ ] **Step 4: Import and restore the processed ICNS after Tauri generation**

Copy the verified source to `assets/branding/app-icon/macos-icon.icns`. Add:

```js
export async function restoreCanonicalMacIcon(sourcePath, outputPath) {
  await copyFile(sourcePath, outputPath);
}
```

Call it after Tauri returns successfully and before favicon copying:

```js
await restoreCanonicalMacIcon(
  join(brandingDirectory, "macos-icon.icns"),
  join(repoRoot, "apps/desktop/src-tauri/icons/icon.icns")
);
```

- [ ] **Step 5: Generate twice and inspect the imported representations**

```bash
pnpm icons:generate
cmp -s assets/branding/app-icon/macos-icon.icns apps/desktop/src-tauri/icons/icon.icns
rm -rf /tmp/QingYu.iconset
iconutil --convert iconset --output /tmp/QingYu.iconset apps/desktop/src-tauri/icons/icon.icns
find /tmp/QingYu.iconset -maxdepth 1 -type f -print | sort
pnpm icons:generate
cmp -s assets/branding/app-icon/macos-icon.icns apps/desktop/src-tauri/icons/icon.icns
```

Expected: both comparisons exit 0; the iconset contains ten RGBA representations from 16×16 through 512×512@2x.

- [ ] **Step 6: Run tests and commit**

```bash
pnpm --filter @markra/scripts test -- src/app-icons/generate.test.mjs
pnpm test
git add assets/branding/app-icon/macos-icon.icns apps/desktop/src-tauri/icons/icon.icns packages/scripts/src/app-icons/generate.mjs packages/scripts/src/app-icons/generate.test.mjs
git commit -m "feat: reuse processed QingYu macOS icon"
```

### Task 3: Wire Every Icon Consumer

**Files:**
- Modify: `apps/desktop/index.html`
- Modify: `apps/web/index.html`
- Modify: `README.md`
- Delete: `apps/desktop/app-icon.svg`

**Interfaces:**
- Consumes: the existing Tauri `icons/icon.icns` path and both generated `public/favicon.png` files.
- Produces: consistent desktop, web, repository, and packaged-app branding without changing the existing `CFBundleIconFile` integration.

- [ ] **Step 1: Wire browser favicons and README branding**

Add inside both HTML `<head>` elements:

```html
<link rel="icon" href="/favicon.png" type="image/png" />
```

Change the README logo to:

```html
<img src="logo.png" width="96" alt="QingYu logo" />
```

Delete `apps/desktop/app-icon.svg` after confirming no remaining references with `rg -n "app-icon.svg" .`.

- [ ] **Step 2: Build both frontends and inspect the generated HTML**

Run:

```bash
pnpm --filter @markra/desktop build
pnpm --filter @markra/web build
rg -n 'favicon.png' apps/desktop/dist/index.html apps/web/dist/index.html
```

Expected: both builds pass and both built HTML files reference `favicon.png`.

- [ ] **Step 3: Commit icon consumer wiring**

```bash
git add apps/desktop/index.html apps/web/index.html README.md apps/desktop/public/favicon.png apps/web/public/favicon.png
git rm apps/desktop/app-icon.svg
git commit -m "feat: use refreshed icon across app surfaces"
```

### Task 4: Automated Asset Verification

**Files:**
- Create: `packages/scripts/src/app-icons/verify.mjs`
- Create: `packages/scripts/src/app-icons/verify.test.mjs`
- Modify: `package.json`

**Interfaces:**
- Consumes: generated icon tree and optional built `.app` path.
- Produces: `verifyIconAssets(repoRoot: string): Promise<void>` and a root `icons:verify` command that fails on missing sizes, incorrect alpha, or absent macOS assets.

- [ ] **Step 1: Write a failing ICO-directory verification test**

```js
import { expect, test } from "vitest";
import { assertRequiredIcoSizes } from "./verify.mjs";

function makeIcoDirectory(sizes) {
  const buffer = Buffer.alloc(6 + sizes.length * 16);
  buffer.writeUInt16LE(0, 0);
  buffer.writeUInt16LE(1, 2);
  buffer.writeUInt16LE(sizes.length, 4);
  sizes.forEach((size, index) => {
    const offset = 6 + index * 16;
    buffer.writeUInt8(size === 256 ? 0 : size, offset);
    buffer.writeUInt8(size === 256 ? 0 : size, offset + 1);
  });
  return buffer;
}

test("assertRequiredIcoSizes rejects an ICO without a 24px layer", () => {
  expect(() => assertRequiredIcoSizes(makeIcoDirectory([16, 32, 48, 64, 256]))).toThrow("icon.ico is missing 24x24");
});

test("assertRequiredIcoSizes accepts every required Windows layer", () => {
  expect(() => assertRequiredIcoSizes(makeIcoDirectory([16, 24, 32, 48, 64, 256]))).not.toThrow();
});
```

- [ ] **Step 2: Run the test and confirm the verifier is missing**

Run: `pnpm --filter @markra/scripts test -- src/app-icons/verify.test.mjs`

Expected: FAIL because `verify.mjs` does not exist.

- [ ] **Step 3: Implement verification for every committed platform family**

```js
import { access, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const requiredIcoSizes = [16, 24, 32, 48, 64, 256];
const squareLogos = [30, 44, 71, 89, 107, 142, 150, 284, 310].map((size) => `Square${size}x${size}Logo.png`);
const androidDensities = ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"];
const iosFiles = [
  "AppIcon-20x20@1x.png", "AppIcon-20x20@2x-1.png", "AppIcon-20x20@2x.png", "AppIcon-20x20@3x.png",
  "AppIcon-29x29@1x.png", "AppIcon-29x29@2x-1.png", "AppIcon-29x29@2x.png", "AppIcon-29x29@3x.png",
  "AppIcon-40x40@1x.png", "AppIcon-40x40@2x-1.png", "AppIcon-40x40@2x.png", "AppIcon-40x40@3x.png",
  "AppIcon-60x60@2x.png", "AppIcon-60x60@3x.png", "AppIcon-76x76@1x.png", "AppIcon-76x76@2x.png",
  "AppIcon-83.5x83.5@2x.png", "AppIcon-512@2x.png"
];

export function assertRequiredIcoSizes(buffer) {
  if (buffer.readUInt16LE(2) !== 1) throw new Error("icon.ico has an invalid type");
  const count = buffer.readUInt16LE(4);
  const sizes = new Set();
  for (let index = 0; index < count; index += 1) {
    const offset = 6 + index * 16;
    const width = buffer.readUInt8(offset) || 256;
    const height = buffer.readUInt8(offset + 1) || 256;
    if (width === height) sizes.add(width);
  }
  for (const size of requiredIcoSizes) {
    if (!sizes.has(size)) throw new Error(`icon.ico is missing ${size}x${size}`);
  }
}

async function requireFile(path) {
  await access(path);
}

async function requirePng(path, width, height, alpha) {
  const metadata = await sharp(path).metadata();
  if (metadata.format !== "png" || metadata.width !== width || metadata.height !== height) {
    throw new Error(`${path} must be a ${width}x${height} PNG`);
  }
  if (alpha !== undefined && metadata.hasAlpha !== alpha) {
    throw new Error(`${path} alpha must be ${alpha}`);
  }
}

export async function verifyIconAssets(repoRoot) {
  const iconDirectory = join(repoRoot, "apps/desktop/src-tauri/icons");
  await requirePng(join(iconDirectory, "32x32.png"), 32, 32, true);
  await requirePng(join(iconDirectory, "128x128.png"), 128, 128, true);
  await requirePng(join(iconDirectory, "128x128@2x.png"), 256, 256, true);
  await requirePng(join(iconDirectory, "icon.png"), 512, 512, true);
  assertRequiredIcoSizes(await readFile(join(iconDirectory, "icon.ico")));
  for (const fileName of [...squareLogos, "StoreLogo.png"]) await requireFile(join(iconDirectory, fileName));
  for (const density of androidDensities) {
    for (const fileName of ["ic_launcher.png", "ic_launcher_round.png", "ic_launcher_foreground.png"]) {
      await requireFile(join(iconDirectory, "android", `mipmap-${density}`, fileName));
    }
  }
  await requireFile(join(iconDirectory, "android/mipmap-anydpi-v26/ic_launcher.xml"));
  await requireFile(join(iconDirectory, "android/values/ic_launcher_background.xml"));
  for (const fileName of iosFiles) await requireFile(join(iconDirectory, "ios", fileName));
  await requirePng(join(iconDirectory, "ios/AppIcon-512@2x.png"), 1024, 1024, false);
  const canonicalMacIcon = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));
  const packagedMacIcon = await readFile(join(iconDirectory, "icon.icns"));
  if (!canonicalMacIcon.equals(packagedMacIcon)) {
    throw new Error("icons/icon.icns must match the imported canonical macOS icon");
  }
  await requirePng(join(repoRoot, "apps/desktop/public/favicon.png"), 32, 32, true);
  await requirePng(join(repoRoot, "apps/web/public/favicon.png"), 32, 32, true);
}

const currentFile = fileURLToPath(import.meta.url);
if (process.argv[1] && resolve(process.argv[1]) === currentFile) {
  verifyIconAssets(resolve(dirname(currentFile), "../../../..")).then(
    () => console.log("[icons:verify] all icon assets are valid"),
    (error) => { console.error(error); process.exit(1); }
  );
}
```

Expose it through:

```json
"icons:verify": "node packages/scripts/src/app-icons/verify.mjs"
```

- [ ] **Step 4: Run focused and real-asset verification**

```bash
pnpm --filter @markra/scripts test -- src/app-icons/verify.test.mjs
pnpm icons:verify
```

Expected: both commands pass.

- [ ] **Step 5: Commit the verifier**

```bash
git add package.json packages/scripts/src/app-icons/verify.mjs packages/scripts/src/app-icons/verify.test.mjs
git commit -m "test: verify cross-platform icon assets"
```

### Task 5: Package And Live macOS 26 Acceptance

**Files:**
- Verify only; no source changes expected.

**Interfaces:**
- Consumes: all earlier outputs.
- Produces: evidence that the built QingYu application uses QingYu's processed ICNS through the existing `CFBundleIconFile` path on macOS 26.

- [ ] **Step 1: Run repository verification**

```bash
pnpm --filter @markra/scripts test -- src/app-icons
pnpm icons:verify
pnpm build
```

Expected: every command exits 0.

- [ ] **Step 2: Build a debug macOS application bundle**

Run: `pnpm tauri build --debug --bundles app`

Expected: a debug `QingYu.app` is produced under `apps/desktop/src-tauri/target/debug/bundle/macos/`.

- [ ] **Step 3: Inspect the built bundle**

```bash
APP="apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app"
plutil -p "$APP/Contents/Info.plist" | rg 'CFBundleIconFile|icon.icns'
test -f "$APP/Contents/Resources/icon.icns"
cmp -s assets/branding/app-icon/macos-icon.icns "$APP/Contents/Resources/icon.icns"
codesign --verify --deep --strict "$APP"
```

Expected: `CFBundleIconFile` references `icon.icns`, the bundled ICNS is byte-identical to the imported canonical asset, and code-sign verification succeeds.

- [ ] **Step 4: Verify live Finder and Dock rendering on macOS 26.5.2**

Copy the debug app to a temporary unique name, register it with Launch Services, launch it, and inspect Finder and Dock:

```bash
rm -rf /tmp/QingYu-Icon-QA.app
cp -R "apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app" /tmp/QingYu-Icon-QA.app
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /tmp/QingYu-Icon-QA.app
open /tmp/QingYu-Icon-QA.app
```

Confirm Finder and Dock keep the feather centered, uncropped, and recognizable, with no white outer square or double-rounded border. Quit the QA copy and remove `/tmp/QingYu-Icon-QA.app` after inspection.

- [ ] **Step 5: Confirm worktree hygiene**

Run:

```bash
git status --short
git diff --check
git log -6 --oneline
```

Expected: the worktree is clean; no generated `dist`, `target`, temporary asset catalog, or alternate lockfile is tracked.
