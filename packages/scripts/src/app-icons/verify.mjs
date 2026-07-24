import { readFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const currentFile = fileURLToPath(import.meta.url);
const currentDirectory = dirname(currentFile);
const requiredIcoSizes = [16, 24, 32, 48, 64, 256];
const argbIcnsRepresentations = new Map([
  ["ic04", 16],
  ["ic05", 32]
]);
const pngIcnsRepresentations = new Map([
  ["ic07", 128],
  ["ic08", 256],
  ["ic09", 512],
  ["ic10", 1024],
  ["ic11", 32],
  ["ic12", 64],
  ["ic13", 256],
  ["ic14", 512]
]);
const requiredIcnsChunkTypes = [...argbIcnsRepresentations.keys(), ...pngIcnsRepresentations.keys()];
const pngSignature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

const windowsPngs = new Map([
  ["Square30x30Logo.png", 30],
  ["Square44x44Logo.png", 44],
  ["Square71x71Logo.png", 71],
  ["Square89x89Logo.png", 89],
  ["Square107x107Logo.png", 107],
  ["Square142x142Logo.png", 142],
  ["Square150x150Logo.png", 150],
  ["Square284x284Logo.png", 284],
  ["Square310x310Logo.png", 310],
  ["StoreLogo.png", 50]
]);

const iosPngs = new Map([
  ["AppIcon-20x20@1x.png", 20],
  ["AppIcon-20x20@2x-1.png", 40],
  ["AppIcon-20x20@2x.png", 40],
  ["AppIcon-20x20@3x.png", 60],
  ["AppIcon-29x29@1x.png", 29],
  ["AppIcon-29x29@2x-1.png", 58],
  ["AppIcon-29x29@2x.png", 58],
  ["AppIcon-29x29@3x.png", 87],
  ["AppIcon-40x40@1x.png", 40],
  ["AppIcon-40x40@2x-1.png", 80],
  ["AppIcon-40x40@2x.png", 80],
  ["AppIcon-40x40@3x.png", 120],
  ["AppIcon-60x60@2x.png", 120],
  ["AppIcon-60x60@3x.png", 180],
  ["AppIcon-76x76@1x.png", 76],
  ["AppIcon-76x76@2x.png", 152],
  ["AppIcon-83.5x83.5@2x.png", 167],
  ["AppIcon-512@2x.png", 1024]
]);

const androidDensities = new Map([
  ["mdpi", { scale: 1, launcherSize: 48 }],
  ["hdpi", { scale: 1.5, launcherSize: 72 }],
  ["xhdpi", { scale: 2, launcherSize: 96 }],
  ["xxhdpi", { scale: 3, launcherSize: 144 }],
  ["xxxhdpi", { scale: 4, launcherSize: 192 }]
]);

function displayPath(filePath, repoRoot) {
  return repoRoot ? relative(repoRoot, filePath) : filePath;
}

export function assertRequiredIcoSizes(buffer) {
  if (!Buffer.isBuffer(buffer) || buffer.length < 6) throw new Error("icon.ico header is truncated");
  if (buffer.readUInt16LE(0) !== 0) throw new Error("icon.ico has an invalid reserved field");
  if (buffer.readUInt16LE(2) !== 1) throw new Error("icon.ico has an invalid type");

  const count = buffer.readUInt16LE(4);
  if (count === 0) throw new Error("icon.ico has no image entries");
  const directoryEnd = 6 + count * 16;
  if (directoryEnd > buffer.length) throw new Error("icon.ico directory is truncated");

  const sizes = new Set();
  for (let index = 0; index < count; index += 1) {
    const offset = 6 + index * 16;
    const width = buffer.readUInt8(offset) || 256;
    const height = buffer.readUInt8(offset + 1) || 256;
    const payloadLength = buffer.readUInt32LE(offset + 8);
    const payloadOffset = buffer.readUInt32LE(offset + 12);
    const payloadEnd = payloadOffset + payloadLength;
    if (width !== height) throw new Error(`icon.ico entry ${index + 1} is not square`);
    if (payloadLength === 0 || payloadOffset < directoryEnd || payloadEnd > buffer.length || payloadEnd < payloadOffset) {
      throw new Error(`icon.ico entry ${index + 1} payload is out of bounds`);
    }
    sizes.add(width);
  }

  for (const size of requiredIcoSizes) {
    if (!sizes.has(size)) throw new Error(`icon.ico is missing ${size}x${size}`);
  }
}

export async function assertPng(filePath, expected, repoRoot) {
  const label = displayPath(filePath, repoRoot);
  let metadata;
  try {
    metadata = await sharp(filePath).metadata();
  } catch (error) {
    throw new Error(`${label} must be a readable PNG: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (metadata.format !== "png" || metadata.width !== expected.width || metadata.height !== expected.height) {
    throw new Error(`${label} must be a ${expected.width}x${expected.height} PNG`);
  }
  if (metadata.channels !== expected.channels) {
    throw new Error(`${label} must use ${expected.channels} channels`);
  }
  const shouldHaveAlpha = expected.channels === 4;
  if (metadata.hasAlpha !== shouldHaveAlpha) {
    throw new Error(`${label} alpha must be ${shouldHaveAlpha}`);
  }
}

async function assertIosRendering(filePath, iosMasterPath, size, repoRoot) {
  await assertPng(filePath, { width: size, height: size, channels: 3 }, repoRoot);
  const [actual, expected] = await Promise.all([
    sharp(filePath).raw().toBuffer(),
    sharp(iosMasterPath)
      .resize(size, size, { fit: "fill", kernel: "lanczos3" })
      .removeAlpha()
      .raw()
      .toBuffer()
  ]);
  if (!actual.equals(expected)) {
    throw new Error(`${displayPath(filePath, repoRoot)} must be rendered from the full-bleed iOS master`);
  }
}

function assertValidArgbRle(payload, size, label, chunkType) {
  const fail = () => {
    throw new Error(`${label} ${chunkType} has invalid ARGB/RLE data for ${size}x${size}`);
  };
  if (payload.length <= 4 || payload.toString("ascii", 0, 4) !== "ARGB") fail();

  const pixelsPerChannel = size * size;
  let offset = 4;
  for (let channel = 0; channel < 4; channel += 1) {
    let decodedPixels = 0;
    while (decodedPixels < pixelsPerChannel) {
      if (offset >= payload.length) fail();
      const control = payload[offset];
      offset += 1;
      if (control < 128) {
        const literalLength = control + 1;
        if (offset + literalLength > payload.length) fail();
        offset += literalLength;
        decodedPixels += literalLength;
      } else {
        const repeatLength = control - 125;
        if (offset >= payload.length) fail();
        offset += 1;
        decodedPixels += repeatLength;
      }
      if (decodedPixels > pixelsPerChannel) fail();
    }
  }
  if (offset !== payload.length) fail();
}

async function assertDecodedPngRepresentation(payload, size, label, chunkType) {
  const fail = () => {
    throw new Error(`${label} ${chunkType} must decode as a ${size}x${size} PNG`);
  };
  if (payload.length < pngSignature.length || !payload.subarray(0, pngSignature.length).equals(pngSignature)) fail();
  let info;
  try {
    const decoded = await sharp(payload, { failOn: "error" }).raw().toBuffer({ resolveWithObject: true });
    info = decoded.info;
  } catch {
    fail();
  }
  if (info.width !== size || info.height !== size) fail();
  if (info.channels !== 4) throw new Error(`${label} ${chunkType} must decode as a ${size}x${size} RGBA PNG`);
}

export async function assertValidIcns(buffer, label = "icon.icns") {
  if (!Buffer.isBuffer(buffer) || buffer.length < 8 || buffer.toString("ascii", 0, 4) !== "icns") {
    throw new Error(`${label} has an invalid header`);
  }
  if (buffer.readUInt32BE(4) !== buffer.length) throw new Error(`${label} has an invalid container length`);

  const chunks = new Map();
  let offset = 8;
  while (offset < buffer.length) {
    if (offset + 8 > buffer.length) throw new Error(`${label} has a truncated chunk header`);
    const chunkType = buffer.toString("ascii", offset, offset + 4);
    const chunkLength = buffer.readUInt32BE(offset + 4);
    if (chunkLength < 8 || offset + chunkLength > buffer.length) {
      throw new Error(`${label} has an invalid ${chunkType} chunk`);
    }
    if (chunks.has(chunkType) && requiredIcnsChunkTypes.includes(chunkType)) {
      throw new Error(`${label} has a duplicate ${chunkType} representation`);
    }
    chunks.set(chunkType, buffer.subarray(offset + 8, offset + chunkLength));
    offset += chunkLength;
  }
  for (const chunkType of requiredIcnsChunkTypes) {
    if (!chunks.has(chunkType)) throw new Error(`${label} is missing the ${chunkType} representation`);
    if (chunks.get(chunkType).length === 0) throw new Error(`${label} ${chunkType} representation is empty`);
  }
  for (const [chunkType, size] of argbIcnsRepresentations) {
    assertValidArgbRle(chunks.get(chunkType), size, label, chunkType);
  }
  for (const [chunkType, size] of pngIcnsRepresentations) {
    await assertDecodedPngRepresentation(chunks.get(chunkType), size, label, chunkType);
  }
}

async function assertAlphaInsideSafeRegion(filePath, scale, repoRoot) {
  const label = displayPath(filePath, repoRoot);
  const expectedSize = Math.round(108 * scale);
  await assertPng(filePath, { width: expectedSize, height: expectedSize, channels: 4 }, repoRoot);
  const { data, info } = await sharp(filePath).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  let minX = info.width;
  let minY = info.height;
  let maxX = -1;
  let maxY = -1;
  for (let index = 0; index < info.width * info.height; index += 1) {
    if (data[index * 4 + 3] === 0) continue;
    const x = index % info.width;
    const y = Math.floor(index / info.width);
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  if (maxX < minX || maxY < minY) throw new Error(`${label} has no visible foreground pixels`);

  const safeMinimum = Math.ceil(21 * scale);
  const safeMaximum = Math.ceil(87 * scale) - 1;
  if (minX < safeMinimum || minY < safeMinimum || maxX > safeMaximum || maxY > safeMaximum) {
    throw new Error(`${label} visible bounds must stay inside ${safeMinimum}..${safeMaximum}`);
  }
}

async function assertAndroidMetadata(repoRoot, androidDirectory) {
  const manifestPath = join(repoRoot, "assets/branding/app-icon/icon-manifest.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const expectedManifest = {
    default: "platform-master.png",
    bg_color: "#F9B52B",
    android_fg: "android-foreground.png",
    android_fg_scale: 100
  };
  if (JSON.stringify(manifest) !== JSON.stringify(expectedManifest)) {
    throw new Error("assets/branding/app-icon/icon-manifest.json must preserve the adaptive icon metadata");
  }

  const adaptiveXml = await readFile(join(androidDirectory, "mipmap-anydpi-v26/ic_launcher.xml"), "utf8");
  if (!/<foreground\s+android:drawable="@mipmap\/ic_launcher_foreground"\s*\/>/.test(adaptiveXml)) {
    throw new Error("Android adaptive icon XML must reference @mipmap/ic_launcher_foreground");
  }
  if (!/<background\s+android:drawable="@color\/ic_launcher_background"\s*\/>/.test(adaptiveXml)) {
    throw new Error("Android adaptive icon XML must reference @color/ic_launcher_background");
  }

  const backgroundXml = await readFile(join(androidDirectory, "values/ic_launcher_background.xml"), "utf8");
  if (!/<color\s+name="ic_launcher_background">#F9B52B<\/color>/.test(backgroundXml)) {
    throw new Error("Android launcher background must be #F9B52B");
  }
}

export async function verifyIconAssets(repoRoot) {
  const root = resolve(repoRoot);
  const iconDirectory = join(root, "apps/desktop/src-tauri/icons");
  const linuxPngs = new Map([
    ["32x32.png", 32],
    ["64x64.png", 64],
    ["128x128.png", 128],
    ["128x128@2x.png", 256],
    ["icon.png", 512]
  ]);
  for (const [fileName, size] of linuxPngs) {
    await assertPng(join(iconDirectory, fileName), { width: size, height: size, channels: 4 }, root);
  }

  assertRequiredIcoSizes(await readFile(join(iconDirectory, "icon.ico")));
  for (const [fileName, size] of windowsPngs) {
    await assertPng(join(iconDirectory, fileName), { width: size, height: size, channels: 4 }, root);
  }

  const androidDirectory = join(iconDirectory, "android");
  for (const [density, { scale, launcherSize }] of androidDensities) {
    const densityDirectory = join(androidDirectory, `mipmap-${density}`);
    await assertPng(join(densityDirectory, "ic_launcher.png"), {
      width: launcherSize,
      height: launcherSize,
      channels: 4
    }, root);
    await assertPng(join(densityDirectory, "ic_launcher_round.png"), {
      width: launcherSize,
      height: launcherSize,
      channels: 4
    }, root);
    await assertAlphaInsideSafeRegion(join(densityDirectory, "ic_launcher_foreground.png"), scale, root);
  }
  await assertAndroidMetadata(root, androidDirectory);

  const iosDirectory = join(iconDirectory, "ios");
  const iosMasterPath = join(root, "assets/branding/app-icon/ios-master.png");
  await assertPng(iosMasterPath, { width: 1024, height: 1024, channels: 3 }, root);
  for (const [fileName, size] of iosPngs) {
    await assertIosRendering(join(iosDirectory, fileName), iosMasterPath, size, root);
  }

  const canonicalMacPath = join(root, "assets/branding/app-icon/macos-icon.icns");
  const packagedMacPath = join(iconDirectory, "icon.icns");
  const canonicalMacIcon = await readFile(canonicalMacPath);
  const packagedMacIcon = await readFile(packagedMacPath);
  await assertValidIcns(canonicalMacIcon, displayPath(canonicalMacPath, root));
  await assertValidIcns(packagedMacIcon, displayPath(packagedMacPath, root));
  if (!canonicalMacIcon.equals(packagedMacIcon)) {
    throw new Error("apps/desktop/src-tauri/icons/icon.icns must match the imported canonical macOS icon");
  }

  await assertPng(join(root, "apps/desktop/public/favicon.png"), { width: 32, height: 32, channels: 4 }, root);
  await assertPng(join(root, "apps/web/public/favicon.png"), { width: 32, height: 32, channels: 4 }, root);
}

if (process.argv[1] && resolve(process.argv[1]) === currentFile) {
  verifyIconAssets(resolve(currentDirectory, "../../../..")).then(
    () => console.log("[icons:verify] all icon assets are valid"),
    (error) => {
      console.error(error);
      process.exit(1);
    }
  );
}
