import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import sharp from "sharp";
import * as generator from "./generate.mjs";

const { buildIconLayers } = generator;
const currentDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(currentDirectory, "../../../..");

function alphaBounds(data, width, height) {
  let minX = width;
  let minY = height;
  let maxX = -1;
  let maxY = -1;
  for (let index = 0; index < width * height; index += 1) {
    if (data[index * 4 + 3] === 0) continue;
    const x = index % width;
    const y = Math.floor(index / width);
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { minX, minY, maxX, maxY };
}

function expandMask(mask, blockedMask, width, height, radius) {
  let expanded = Uint8Array.from(mask);
  for (let step = 0; step < radius; step += 1) {
    const next = Uint8Array.from(expanded);
    for (let index = 0; index < expanded.length; index += 1) {
      if (!expanded[index]) continue;
      const x = index % width;
      const y = Math.floor(index / width);
      const include = (candidate) => {
        if (!blockedMask[candidate]) next[candidate] = 1;
      };
      if (x > 0) include(index - 1);
      if (x + 1 < width) include(index + 1);
      if (y > 0) include(index - width);
      if (y + 1 < height) include(index + width);
    }
    expanded = next;
  }
  return expanded;
}

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

  test("keeps the connected feather while excluding disconnected bright plate-edge pixels", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icon-components-"));
    const inputPath = join(directory, "source.png");
    const outputDirectory = join(directory, "output");

    await sharp({
      create: { width: 40, height: 40, channels: 4, background: "#ffffff" }
    })
      .composite([{ input: Buffer.from('<svg width="40" height="40"><rect x="4" y="4" width="32" height="32" rx="4" fill="#f9b52b"/><rect x="16" y="10" width="8" height="20" fill="white"/><rect x="7" y="7" width="2" height="2" fill="white"/></svg>') }])
      .png()
      .toFile(inputPath);

    const paths = await buildIconLayers(inputPath, outputDirectory, 40);
    const feather = await sharp(paths.featherPath).ensureAlpha().raw().toBuffer();
    const alpha = (x, y) => feather[(y * 40 + x) * 4 + 3];

    expect(alpha(20, 18)).toBe(255);
    expect(alpha(8, 8)).toBe(0);
  });

  test("smoothly fills the feather area without nearest-pixel stripe seams", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icon-fill-"));
    const inputPath = join(directory, "source.png");
    const outputDirectory = join(directory, "output");
    const size = 64;
    const source = Buffer.alloc(size * size * 4);
    for (let y = 0; y < size; y += 1) {
      for (let x = 0; x < size; x += 1) {
        const offset = (y * size + x) * 4;
        const isPlate = x >= 4 && x < 60 && y >= 4 && y < 60;
        const isFeather = x >= 26 && x < 38 && y >= 12 && y < 52;
        const color = !isPlate || isFeather ? [255, 255, 255] : [249, 140 + Math.floor(y * 1.5), 43];
        source[offset] = color[0];
        source[offset + 1] = color[1];
        source[offset + 2] = color[2];
        source[offset + 3] = 255;
      }
    }
    await sharp(source, { raw: { width: size, height: size, channels: 4 } }).png().toFile(inputPath);

    const paths = await buildIconLayers(inputPath, outputDirectory, size);
    const background = await sharp(paths.backgroundPath).ensureAlpha().raw().toBuffer();
    const greenAt = (x, y) => background[(y * size + x) * 4 + 1];
    let maxVerticalStep = 0;
    for (let y = 13; y < 51; y += 1) {
      maxVerticalStep = Math.max(maxVerticalStep, Math.abs(greenAt(32, y) - greenAt(32, y - 1)));
    }

    expect(maxVerticalStep).toBeLessThanOrEqual(10);
    expect(background[(32 * size + 32) * 4 + 3]).toBe(255);
  });

  test("fully replaces the mixed-color feather edge before tapering the blend band", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icon-edge-"));
    const inputPath = join(directory, "source.png");
    const outputDirectory = join(directory, "output");
    const size = 64;
    const plate = [249, 180, 43];
    const source = Buffer.alloc(size * size * 4);
    for (let y = 0; y < size; y += 1) {
      for (let x = 0; x < size; x += 1) {
        const offset = (y * size + x) * 4;
        const isPlate = x >= 4 && x < 60 && y >= 4 && y < 60;
        const isCore = x >= 26 && x < 38 && y >= 12 && y < 52;
        const isMixedEdge = x >= 25 && x < 39 && y >= 11 && y < 53;
        const color = !isPlate || isCore ? [255, 255, 255] : isMixedEdge ? [252, 220, 120] : plate;
        source[offset] = color[0];
        source[offset + 1] = color[1];
        source[offset + 2] = color[2];
        source[offset + 3] = 255;
      }
    }
    await sharp(source, { raw: { width: size, height: size, channels: 4 } }).png().toFile(inputPath);

    const paths = await buildIconLayers(inputPath, outputDirectory, size);
    const background = await sharp(paths.backgroundPath).ensureAlpha().raw().toBuffer();
    const pixel = (x, y) => [...background.subarray((y * size + x) * 4, (y * size + x) * 4 + 3)];

    expect(pixel(25, 32)).toEqual(plate);
    expect(pixel(38, 32)).toEqual(plate);
  });

  test("preserves canonical plate RGB outside the feather fill and minimal blend band", async () => {
    const masterPath = join(repoRoot, "assets/branding/app-icon/platform-master.png");
    const featherPath = join(repoRoot, "assets/branding/app-icon/feather.png");
    const backgroundPath = join(repoRoot, "assets/branding/app-icon/background.png");
    const masterResult = await sharp(masterPath).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    const feather = await sharp(featherPath).ensureAlpha().raw().toBuffer();
    const background = await sharp(backgroundPath).ensureAlpha().raw().toBuffer();
    const { data: master, info } = masterResult;
    const featherMask = new Uint8Array(info.width * info.height);
    const externalMask = new Uint8Array(info.width * info.height);

    for (let index = 0; index < featherMask.length; index += 1) {
      featherMask[index] = feather[index * 4 + 3] === 0 ? 0 : 1;
      externalMask[index] = master[index * 4 + 3] === 0 ? 1 : 0;
    }
    const fillPadding = Math.max(1, Math.round(Math.min(info.width, info.height) / 512));
    const blendRadius = Math.max(2, Math.round(Math.min(info.width, info.height) / 128));
    const fillAndBlendRegion = expandMask(featherMask, externalMask, info.width, info.height, fillPadding + blendRadius);
    let comparedPixels = 0;
    let changedPixels = 0;

    for (let index = 0; index < fillAndBlendRegion.length; index += 1) {
      if (fillAndBlendRegion[index] || externalMask[index]) continue;
      comparedPixels += 1;
      const offset = index * 4;
      if (background[offset] !== master[offset]
        || background[offset + 1] !== master[offset + 1]
        || background[offset + 2] !== master[offset + 2]) {
        changedPixels += 1;
      }
    }

    expect(comparedPixels).toBeGreaterThan(500_000);
    expect(changedPixels).toBe(0);
  });

  test("carries canonical plate texture into the filled feather area", async () => {
    const featherPath = join(repoRoot, "assets/branding/app-icon/feather.png");
    const backgroundPath = join(repoRoot, "assets/branding/app-icon/background.png");
    const result = await sharp(backgroundPath).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    const feather = await sharp(featherPath).ensureAlpha().raw().toBuffer();
    const lowPass = await sharp(result.data, { raw: result.info }).blur(16).raw().toBuffer();
    let residualTotal = 0;
    let featherPixels = 0;

    for (let index = 0; index < result.info.width * result.info.height; index += 1) {
      if (feather[index * 4 + 3] === 0) continue;
      featherPixels += 1;
      for (let channel = 0; channel < 3; channel += 1) {
        residualTotal += Math.abs(result.data[index * 4 + channel] - lowPass[index * 4 + channel]);
      }
    }

    expect(featherPixels).toBeGreaterThan(90_000);
    expect(residualTotal / featherPixels).toBeGreaterThan(1);
  });
});

describe("generated Android adaptive icon", () => {
  test("keeps the mdpi foreground inside the centered 66x66 guaranteed safe region", async () => {
    const foregroundPath = join(repoRoot, "apps/desktop/src-tauri/icons/android/mipmap-mdpi/ic_launcher_foreground.png");
    const { data, info } = await sharp(foregroundPath).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    const bounds = alphaBounds(data, info.width, info.height);

    expect([info.width, info.height]).toEqual([108, 108]);
    expect(bounds.minX).toBeGreaterThanOrEqual(21);
    expect(bounds.minY).toBeGreaterThanOrEqual(21);
    expect(bounds.maxX).toBeLessThanOrEqual(86);
    expect(bounds.maxY).toBeLessThanOrEqual(86);
  });

  test("is generated through Tauri's supported icon manifest fields", async () => {
    const manifestPath = join(repoRoot, "assets/branding/app-icon/icon-manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));

    expect(manifest).toEqual({
      default: "platform-master.png",
      bg_color: "#F9B52B",
      android_fg: "android-foreground.png",
      android_fg_scale: 100
    });
  });
});

describe("repairAndroidHdpiIcons", () => {
  test("derives deterministic 72x72 RGBA legacy launchers from xhdpi", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-android-hdpi-"));
    const xhdpiDirectory = join(directory, "mipmap-xhdpi");
    await mkdir(xhdpiDirectory);

    await sharp({
      create: { width: 96, height: 96, channels: 4, background: { r: 249, g: 181, b: 43, alpha: 1 } }
    })
      .composite([{ input: Buffer.from('<svg width="96" height="96"><path d="M48 12L68 72L48 58L28 84L38 40Z" fill="white"/></svg>') }])
      .png()
      .toFile(join(xhdpiDirectory, "ic_launcher.png"));
    await sharp({
      create: { width: 96, height: 96, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } }
    })
      .composite([{ input: Buffer.from('<svg width="96" height="96"><circle cx="48" cy="48" r="44" fill="#f9b52b"/></svg>') }])
      .png()
      .toFile(join(xhdpiDirectory, "ic_launcher_round.png"));

    await generator.repairAndroidHdpiIcons(directory);
    const regularPath = join(directory, "mipmap-hdpi", "ic_launcher.png");
    const roundPath = join(directory, "mipmap-hdpi", "ic_launcher_round.png");
    const firstRegular = await readFile(regularPath);
    const firstRound = await readFile(roundPath);
    const regularMetadata = await sharp(firstRegular).metadata();
    const roundMetadata = await sharp(firstRound).metadata();

    expect([regularMetadata.width, regularMetadata.height, regularMetadata.channels]).toEqual([72, 72, 4]);
    expect([roundMetadata.width, roundMetadata.height, roundMetadata.channels]).toEqual([72, 72, 4]);

    await generator.repairAndroidHdpiIcons(directory);
    expect(await readFile(regularPath)).toEqual(firstRegular);
    expect(await readFile(roundPath)).toEqual(firstRound);
  });
});

describe("buildIosMaster", () => {
  test("extends the plate treatment to every corner instead of baking in a rounded enclosure", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-ios-icon-"));
    const backgroundPath = join(directory, "background.png");
    const featherPath = join(directory, "feather.png");
    const masterPath = join(directory, "ios-master.png");
    await sharp({
      create: { width: 32, height: 32, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } }
    })
      .composite([{ input: Buffer.from('<svg width="32" height="32"><rect x="4" y="4" width="24" height="24" rx="6" fill="#ffc84a"/></svg>') }])
      .png()
      .toFile(backgroundPath);
    await sharp({
      create: { width: 32, height: 32, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } }
    })
      .composite([{ input: Buffer.from('<svg width="32" height="32"><path d="M16 8L20 22L16 18L12 24L14 14Z" fill="white"/></svg>') }])
      .png()
      .toFile(featherPath);

    await generator.buildIosMaster(backgroundPath, featherPath, masterPath, 32);

    const { data, info } = await sharp(masterPath).raw().toBuffer({ resolveWithObject: true });
    const pixel = (x, y) => [...data.subarray((y * info.width + x) * info.channels, (y * info.width + x) * info.channels + 3)];

    expect(info.channels).toBe(3);
    expect(pixel(0, 0)).toEqual([255, 200, 74]);
    expect(pixel(31, 0)).toEqual([255, 200, 74]);
    expect(pixel(0, 31)).toEqual([255, 200, 74]);
    expect(pixel(31, 31)).toEqual([255, 200, 74]);
    expect(pixel(16, 12)).toEqual([255, 255, 255]);
  });
});

describe("renderIosPngs", () => {
  test("renders every generated iOS size from the full-bleed RGB master", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-ios-render-"));
    const iosDirectory = join(directory, "ios");
    const masterPath = join(directory, "ios-master.png");
    const smallPath = join(iosDirectory, "AppIcon-20.png");
    const largePath = join(iosDirectory, "AppIcon-40.png");
    await mkdir(iosDirectory);
    await sharp({ create: { width: 32, height: 32, channels: 3, background: "#ffc84a" } }).png().toFile(masterPath);
    await sharp({ create: { width: 20, height: 20, channels: 4, background: "#00000000" } }).png().toFile(smallPath);
    await sharp({ create: { width: 40, height: 40, channels: 4, background: "#00000000" } }).png().toFile(largePath);

    await generator.renderIosPngs(iosDirectory, masterPath);

    const small = await sharp(smallPath).raw().toBuffer({ resolveWithObject: true });
    const large = await sharp(largePath).raw().toBuffer({ resolveWithObject: true });
    expect([small.info.width, small.info.height, small.info.channels]).toEqual([20, 20, 3]);
    expect([large.info.width, large.info.height, large.info.channels]).toEqual([40, 40, 3]);
    expect([...small.data.subarray(0, 3)]).toEqual([255, 200, 74]);
    expect([...large.data.subarray(0, 3)]).toEqual([255, 200, 74]);
  });
});

describe("canonicalizeIcns", () => {
  test("sorts complete ICNS chunks into a deterministic byte order", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icns-"));
    const iconPath = join(directory, "icon.icns");
    const chunk = (type, payload) => {
      const header = Buffer.alloc(8);
      header.write(type, 0, "ascii");
      header.writeUInt32BE(8 + payload.length, 4);
      return Buffer.concat([header, payload]);
    };
    const ic10 = chunk("ic10", Buffer.from([10]));
    const ic07 = chunk("ic07", Buffer.from([7]));
    const header = Buffer.alloc(8);
    header.write("icns", 0, "ascii");
    header.writeUInt32BE(8 + ic10.length + ic07.length, 4);
    await writeFile(iconPath, Buffer.concat([header, ic10, ic07]));

    await generator.canonicalizeIcns(iconPath);

    expect(await readFile(iconPath)).toEqual(Buffer.concat([header, ic07, ic10]));
  });
});

describe("restoreCanonicalMacIcon", () => {
  test("copies the imported processed ICNS", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-mac-icon-"));
    const sourcePath = join(directory, "source.icns");
    const outputPath = join(directory, "output.icns");
    const source = Buffer.from("processed-qingyu-icon");
    await writeFile(sourcePath, source);

    await generator.restoreCanonicalMacIcon(sourcePath, outputPath);

    expect(await readFile(outputPath)).toEqual(source);
  });
});
