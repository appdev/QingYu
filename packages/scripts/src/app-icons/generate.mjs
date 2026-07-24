import { copyFile, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const currentDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(currentDirectory, "../../../..");
const plateColor = { r: 249, g: 181, b: 43 };
const androidMdpiSize = 108;
const androidForegroundContentSize = 60;

function isEdgeWhite(r, g, b) {
  return r >= 238 && g >= 238 && b >= 238 && Math.max(r, g, b) - Math.min(r, g, b) <= 18;
}

function isFeatherWhite(r, g, b) {
  return r >= 218 && g >= 218 && b >= 218 && Math.max(r, g, b) - Math.min(r, g, b) <= 28;
}

function nearestValidPixelMap(validMask, width, height) {
  const nearest = new Int32Array(validMask.length).fill(-1);
  const queue = new Uint32Array(validMask.length);
  let head = 0;
  let tail = 0;
  for (let index = 0; index < validMask.length; index += 1) {
    if (!validMask[index]) continue;
    nearest[index] = index;
    queue[tail++] = index;
  }
  const visit = (from, next) => {
    if (nearest[next] !== -1) return;
    nearest[next] = nearest[from];
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
  return nearest;
}

function reflectedCoordinate(value, minimum, maximum) {
  if (minimum === maximum) return minimum;
  let reflected = value;
  while (reflected < minimum || reflected > maximum) {
    if (reflected < minimum) reflected = minimum + (minimum - reflected);
    if (reflected > maximum) reflected = maximum - (reflected - maximum);
  }
  return reflected;
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

export function largestConnectedComponent(mask, width, height) {
  const visited = new Uint8Array(width * height);
  let largest = [];

  for (let start = 0; start < mask.length; start += 1) {
    if (!mask[start] || visited[start]) continue;
    const component = [start];
    visited[start] = 1;
    for (let head = 0; head < component.length; head += 1) {
      const index = component[head];
      const x = index % width;
      const y = Math.floor(index / width);
      const visit = (next) => {
        if (!mask[next] || visited[next]) return;
        visited[next] = 1;
        component.push(next);
      };
      if (x > 0) visit(index - 1);
      if (x + 1 < width) visit(index + 1);
      if (y > 0) visit(index - width);
      if (y + 1 < height) visit(index + width);
    }
    if (component.length > largest.length) largest = component;
  }

  const output = new Uint8Array(width * height);
  for (const index of largest) output[index] = 1;
  return output;
}

async function fillFeatherAreaSmoothly(source, featherMask, externalMask, width, height) {
  const fillPadding = Math.max(1, Math.round(Math.min(width, height) / 512));
  const blendRadius = Math.max(2, Math.round(Math.min(width, height) / 128));
  const fillMask = expandMask(featherMask, externalMask, width, height, fillPadding);
  const fillAndBlendMask = expandMask(fillMask, externalMask, width, height, blendRadius);
  const horizontal = Buffer.alloc(source.length);
  const vertical = Buffer.alloc(source.length);
  const hasHorizontal = new Uint8Array(width * height);
  const hasVertical = new Uint8Array(width * height);
  const interpolate = (target, known, index, startOffset, endOffset, position, span) => {
    const offset = index * 4;
    const ratio = position / span;
    for (let channel = 0; channel < 3; channel += 1) {
      target[offset + channel] = Math.round(source[startOffset + channel] * (1 - ratio) + source[endOffset + channel] * ratio);
    }
    known[index] = 1;
  };

  for (let y = 0; y < height; y += 1) {
    let x = 0;
    while (x < width) {
      if (!fillAndBlendMask[y * width + x]) {
        x += 1;
        continue;
      }
      const start = x;
      while (x < width && fillAndBlendMask[y * width + x]) x += 1;
      const end = x - 1;
      if (start === 0 || end + 1 >= width) continue;
      const leftOffset = (y * width + start - 1) * 4;
      const rightOffset = (y * width + end + 1) * 4;
      if (source[leftOffset + 3] === 0 || source[rightOffset + 3] === 0) continue;
      for (let fillX = start; fillX <= end; fillX += 1) {
        const index = y * width + fillX;
        interpolate(horizontal, hasHorizontal, index, leftOffset, rightOffset, fillX - start + 1, end - start + 2);
      }
    }
  }

  for (let x = 0; x < width; x += 1) {
    let y = 0;
    while (y < height) {
      if (!fillAndBlendMask[y * width + x]) {
        y += 1;
        continue;
      }
      const start = y;
      while (y < height && fillAndBlendMask[y * width + x]) y += 1;
      const end = y - 1;
      if (start === 0 || end + 1 >= height) continue;
      const topOffset = ((start - 1) * width + x) * 4;
      const bottomOffset = ((end + 1) * width + x) * 4;
      if (source[topOffset + 3] === 0 || source[bottomOffset + 3] === 0) continue;
      for (let fillY = start; fillY <= end; fillY += 1) {
        const index = fillY * width + x;
        interpolate(vertical, hasVertical, index, topOffset, bottomOffset, fillY - start + 1, end - start + 2);
      }
    }
  }

  const output = Buffer.from(source);
  for (let index = 0; index < fillAndBlendMask.length; index += 1) {
    if (!fillAndBlendMask[index]) continue;
    const offset = index * 4;
    for (let channel = 0; channel < 3; channel += 1) {
      if (hasHorizontal[index] && hasVertical[index]) {
        output[offset + channel] = Math.round((horizontal[offset + channel] + vertical[offset + channel]) / 2);
      } else if (hasHorizontal[index]) {
        output[offset + channel] = horizontal[offset + channel];
      } else if (hasVertical[index]) {
        output[offset + channel] = vertical[offset + channel];
      } else {
        output[offset + channel] = [plateColor.r, plateColor.g, plateColor.b][channel];
      }
    }
    output[offset + 3] = 255;
  }

  const lowPass = await sharp(output, { raw: { width, height, channels: 4 } })
    .flatten({ background: plateColor })
    .blur(Math.max(1, Math.min(width, height) / 32))
    .raw()
    .toBuffer();
  const validTextureSource = new Uint8Array(fillAndBlendMask.length);
  for (let index = 0; index < fillAndBlendMask.length; index += 1) {
    if (!fillAndBlendMask[index] && source[index * 4 + 3] !== 0) validTextureSource[index] = 1;
  }
  const nearestTextureSource = nearestValidPixelMap(validTextureSource, width, height);
  let regionMinX = width;
  let regionMinY = height;
  let regionMaxX = -1;
  let regionMaxY = -1;
  for (let index = 0; index < fillAndBlendMask.length; index += 1) {
    if (!fillAndBlendMask[index]) continue;
    const x = index % width;
    const y = Math.floor(index / width);
    regionMinX = Math.min(regionMinX, x);
    regionMinY = Math.min(regionMinY, y);
    regionMaxX = Math.max(regionMaxX, x);
    regionMaxY = Math.max(regionMaxY, y);
  }
  const textureMargin = Math.max(1, Math.round(Math.min(width, height) / 64));
  const textureVerticalOffset = Math.max(4, Math.round(Math.min(width, height) * 3 / 16));
  const leftStarts = new Int32Array(height).fill(-1);
  const leftEnds = new Int32Array(height).fill(-1);
  const rightStarts = new Int32Array(height).fill(-1);
  const rightEnds = new Int32Array(height).fill(-1);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < regionMinX; x += 1) {
      if (!validTextureSource[y * width + x]) continue;
      if (leftStarts[y] === -1) leftStarts[y] = x;
      leftEnds[y] = x;
    }
    for (let x = regionMaxX + 1; x < width; x += 1) {
      if (!validTextureSource[y * width + x]) continue;
      if (rightStarts[y] === -1) rightStarts[y] = x;
      rightEnds[y] = x;
    }
    if (leftStarts[y] !== -1) {
      const margin = Math.min(textureMargin, Math.floor((leftEnds[y] - leftStarts[y]) / 4));
      leftStarts[y] += margin;
      leftEnds[y] -= margin;
    }
    if (rightStarts[y] !== -1) {
      const margin = Math.min(textureMargin, Math.floor((rightEnds[y] - rightStarts[y]) / 4));
      rightStarts[y] += margin;
      rightEnds[y] -= margin;
    }
  }
  const blendWeights = new Uint8Array(fillMask.length);
  let expanded = Uint8Array.from(fillMask);
  for (let index = 0; index < fillMask.length; index += 1) {
    if (fillMask[index]) blendWeights[index] = 255;
  }
  for (let step = 1; step <= blendRadius; step += 1) {
    const next = expandMask(expanded, externalMask, width, height, 1);
    const weight = Math.round((255 * (blendRadius - step + 1)) / (blendRadius + 1));
    for (let index = 0; index < next.length; index += 1) {
      if (next[index] && !expanded[index]) blendWeights[index] = weight;
    }
    expanded = next;
  }
  const background = Buffer.from(source);
  for (let index = 0; index < fillAndBlendMask.length; index += 1) {
    if (!fillAndBlendMask[index]) continue;
    const offset = index * 4;
    if (source[offset + 3] === 0) continue;
    const x = index % width;
    const y = Math.floor(index / width);
    const position = regionMaxX === regionMinX ? 0.5 : (x - regionMinX) / (regionMaxX - regionMinX);
    const leftY = reflectedCoordinate(y - textureVerticalOffset, regionMinY, regionMaxY);
    const rightY = reflectedCoordinate(y + textureVerticalOffset, regionMinY, regionMaxY);
    const textureSamples = [];
    if (leftStarts[leftY] !== -1) {
      const sampleX = Math.round(leftStarts[leftY] + position * (leftEnds[leftY] - leftStarts[leftY]));
      textureSamples.push({ index: leftY * width + sampleX, weight: 1 - position * 0.7 });
    }
    if (rightStarts[rightY] !== -1) {
      const sampleX = Math.round(rightEnds[rightY] - position * (rightEnds[rightY] - rightStarts[rightY]));
      textureSamples.push({ index: rightY * width + sampleX, weight: 0.3 + position * 0.7 });
    }
    if (textureSamples.length === 0 && nearestTextureSource[index] !== -1) {
      textureSamples.push({ index: nearestTextureSource[index], weight: 1 });
    }
    const weight = blendWeights[index] / 255;
    for (let channel = 0; channel < 3; channel += 1) {
      let residual = 0;
      let residualWeight = 0;
      for (const sample of textureSamples) {
        residual += (source[sample.index * 4 + channel] - lowPass[sample.index * 3 + channel]) * sample.weight;
        residualWeight += sample.weight;
      }
      const textureResidual = residualWeight === 0 ? 0 : (residual * 1.5) / residualWeight;
      const texturedFill = Math.max(0, Math.min(255, Math.round(lowPass[index * 3 + channel] + textureResidual)));
      background[offset + channel] = Math.round(source[offset + channel] * (1 - weight) + texturedFill * weight);
    }
    background[offset + 3] = 255;
  }
  return background;
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

export async function buildIconLayers(inputPath, outputDirectory, size = 1024) {
  const { data, info } = await sharp(inputPath).resize(size, size, { fit: "fill" }).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const externalMask = edgeConnectedWhiteMask(data, info.width, info.height);
  const featherCandidates = new Uint8Array(info.width * info.height);
  const master = Buffer.from(data);

  for (let index = 0; index < info.width * info.height; index += 1) {
    const offset = index * 4;
    if (externalMask[index]) master[offset + 3] = 0;
    if (!externalMask[index] && isFeatherWhite(data[offset], data[offset + 1], data[offset + 2])) {
      featherCandidates[index] = 1;
    }
  }

  const featherMask = largestConnectedComponent(featherCandidates, info.width, info.height);
  const feather = Buffer.alloc(data.length);
  for (let index = 0; index < info.width * info.height; index += 1) {
    if (featherMask[index]) {
      const offset = index * 4;
      feather[offset] = data[offset];
      feather[offset + 1] = data[offset + 1];
      feather[offset + 2] = data[offset + 2];
      feather[offset + 3] = 255;
    }
  }

  const background = await fillFeatherAreaSmoothly(master, featherMask, externalMask, info.width, info.height);
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

export async function buildAndroidForeground(featherPath, outputPath, size = 1024) {
  const { data, info } = await sharp(featherPath).resize(size, size, { fit: "fill" }).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
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
  if (maxX < minX || maxY < minY) throw new Error(`Android foreground has no visible pixels: ${featherPath}`);

  const cropWidth = maxX - minX + 1;
  const cropHeight = maxY - minY + 1;
  const targetSize = Math.floor((size * androidForegroundContentSize) / androidMdpiSize);
  const resized = await sharp(data, { raw: info })
    .extract({ left: minX, top: minY, width: cropWidth, height: cropHeight })
    .resize(targetSize, targetSize, { fit: "inside" })
    .png()
    .toBuffer({ resolveWithObject: true });
  await sharp({ create: { width: size, height: size, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
    .composite([{ input: resized.data, left: Math.floor((size - resized.info.width) / 2), top: Math.floor((size - resized.info.height) / 2) }])
    .png()
    .toFile(outputPath);
}

export function tauriIconManifest() {
  return {
    default: "platform-master.png",
    bg_color: "#F9B52B",
    android_fg: "android-foreground.png",
    android_fg_scale: 100
  };
}

export async function repairAndroidHdpiIcons(androidDirectory) {
  const sourceDirectory = join(androidDirectory, "mipmap-xhdpi");
  const outputDirectory = join(androidDirectory, "mipmap-hdpi");
  await mkdir(outputDirectory, { recursive: true });

  for (const iconName of ["ic_launcher.png", "ic_launcher_round.png"]) {
    await sharp(join(sourceDirectory, iconName))
      .ensureAlpha()
      .resize(72, 72, { fit: "fill", kernel: "lanczos3" })
      .png()
      .toFile(join(outputDirectory, iconName));
  }
}

function largestCenteredOpaqueSquare(data, info) {
  const stride = info.width + 1;
  const transparentPrefix = new Uint32Array((info.width + 1) * (info.height + 1));
  for (let y = 1; y <= info.height; y += 1) {
    let transparentInRow = 0;
    for (let x = 1; x <= info.width; x += 1) {
      const alpha = data[((y - 1) * info.width + x - 1) * info.channels + 3];
      if (alpha !== 255) transparentInRow += 1;
      transparentPrefix[y * stride + x] = transparentPrefix[(y - 1) * stride + x] + transparentInRow;
    }
  }

  const transparentCount = (left, top, size) => {
    const right = left + size;
    const bottom = top + size;
    return transparentPrefix[bottom * stride + right]
      - transparentPrefix[top * stride + right]
      - transparentPrefix[bottom * stride + left]
      + transparentPrefix[top * stride + left];
  };

  for (let size = Math.min(info.width, info.height); size > 0; size -= 1) {
    const left = Math.floor((info.width - size) / 2);
    const top = Math.floor((info.height - size) / 2);
    if (transparentCount(left, top, size) === 0) return { left, top, width: size, height: size };
  }
  throw new Error("iOS background has no opaque center crop");
}

export async function buildIosMaster(backgroundPath, featherPath, outputPath, size = 1024) {
  const background = await sharp(backgroundPath).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const crop = largestCenteredOpaqueSquare(background.data, background.info);
  const fullBleedBackground = await sharp(background.data, { raw: background.info })
    .extract(crop)
    .resize(size, size, { fit: "fill", kernel: "lanczos3" })
    .removeAlpha()
    .png()
    .toBuffer();
  const feather = await sharp(featherPath)
    .resize(size, size, { fit: "fill", kernel: "lanczos3" })
    .ensureAlpha()
    .png()
    .toBuffer();
  await sharp(fullBleedBackground)
    .composite([{ input: feather }])
    .removeAlpha()
    .png()
    .toFile(outputPath);
}

export async function renderIosPngs(iosDirectory, iosMasterPath) {
  const iconNames = (await readdir(iosDirectory))
    .filter((name) => name.toLowerCase().endsWith(".png"))
    .sort();

  for (const iconName of iconNames) {
    const iconPath = join(iosDirectory, iconName);
    const metadata = await sharp(iconPath).metadata();
    if (!metadata.width || !metadata.height) throw new Error(`Invalid generated iOS icon: ${iconPath}`);
    await sharp(iosMasterPath)
      .resize(metadata.width, metadata.height, { fit: "fill", kernel: "lanczos3" })
      .removeAlpha()
      .png()
      .toFile(iconPath);
  }
}

export async function canonicalizeIcns(iconPath) {
  const source = await readFile(iconPath);
  if (source.length < 8 || source.toString("ascii", 0, 4) !== "icns" || source.readUInt32BE(4) !== source.length) {
    throw new Error(`Invalid ICNS container: ${iconPath}`);
  }

  const chunks = [];
  let offset = 8;
  while (offset < source.length) {
    if (offset + 8 > source.length) throw new Error(`Invalid ICNS chunk header: ${iconPath}`);
    const chunkLength = source.readUInt32BE(offset + 4);
    if (chunkLength < 8 || offset + chunkLength > source.length) throw new Error(`Invalid ICNS chunk length: ${iconPath}`);
    chunks.push(source.subarray(offset, offset + chunkLength));
    offset += chunkLength;
  }
  chunks.sort((left, right) => Buffer.compare(left, right));
  await writeFile(iconPath, Buffer.concat([source.subarray(0, 8), ...chunks]));
}

export async function restoreCanonicalMacIcon(sourcePath, outputPath) {
  await copyFile(sourcePath, outputPath);
}

async function main() {
  const source = join(repoRoot, "logo.png");
  const brandingDirectory = join(repoRoot, "assets/branding/app-icon");
  const paths = await buildIconLayers(source, brandingDirectory);
  const iosMasterPath = join(brandingDirectory, "ios-master.png");
  const androidForegroundPath = join(brandingDirectory, "android-foreground.png");
  const manifestPath = join(brandingDirectory, "icon-manifest.json");
  await buildIosMaster(paths.backgroundPath, paths.featherPath, iosMasterPath);
  await buildAndroidForeground(paths.featherPath, androidForegroundPath);
  await writeFile(manifestPath, `${JSON.stringify(tauriIconManifest(), null, 2)}\n`);
  const result = spawnSync("pnpm", ["--filter", "@markra/desktop", "tauri", "icon", manifestPath], { cwd: repoRoot, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  await repairAndroidHdpiIcons(join(repoRoot, "apps/desktop/src-tauri/icons/android"));
  await canonicalizeIcns(join(repoRoot, "apps/desktop/src-tauri/icons/icon.icns"));
  await renderIosPngs(join(repoRoot, "apps/desktop/src-tauri/icons/ios"), iosMasterPath);
  await restoreCanonicalMacIcon(
    join(brandingDirectory, "macos-icon.icns"),
    join(repoRoot, "apps/desktop/src-tauri/icons/icon.icns")
  );
  await mkdir(join(repoRoot, "apps/desktop/public"), { recursive: true });
  await mkdir(join(repoRoot, "apps/web/public"), { recursive: true });
  await copyFile(join(repoRoot, "apps/desktop/src-tauri/icons/32x32.png"), join(repoRoot, "apps/desktop/public/favicon.png"));
  await copyFile(join(repoRoot, "apps/desktop/src-tauri/icons/32x32.png"), join(repoRoot, "apps/web/public/favicon.png"));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch((error) => { console.error(error); process.exit(1); });
