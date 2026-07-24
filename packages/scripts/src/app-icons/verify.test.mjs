import { cp, mkdir, mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import sharp from "sharp";
import {
  assertPng,
  assertRequiredIcoSizes,
  assertValidIcns,
  verifyIconAssets
} from "./verify.mjs";

const currentDirectory = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(currentDirectory, "../../../..");

function makeIcoDirectory(sizes) {
  const directoryLength = 6 + sizes.length * 16;
  const payloadLength = 8;
  const buffer = Buffer.alloc(directoryLength + sizes.length * payloadLength);
  buffer.writeUInt16LE(0, 0);
  buffer.writeUInt16LE(1, 2);
  buffer.writeUInt16LE(sizes.length, 4);
  sizes.forEach((size, index) => {
    const offset = 6 + index * 16;
    buffer.writeUInt8(size === 256 ? 0 : size, offset);
    buffer.writeUInt8(size === 256 ? 0 : size, offset + 1);
    buffer.writeUInt16LE(1, offset + 4);
    buffer.writeUInt16LE(32, offset + 6);
    buffer.writeUInt32LE(payloadLength, offset + 8);
    buffer.writeUInt32LE(directoryLength + index * payloadLength, offset + 12);
  });
  return buffer;
}

function readIcnsChunks(buffer) {
  const chunks = [];
  let offset = 8;
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset + 4);
    chunks.push({
      type: buffer.toString("ascii", offset, offset + 4),
      payload: buffer.subarray(offset + 8, offset + length)
    });
    offset += length;
  }
  return chunks;
}

function makeIcns(chunks) {
  const encodedChunks = chunks.map(({ type, payload }) => {
    const chunk = Buffer.alloc(8 + payload.length);
    chunk.write(type, 0, "ascii");
    chunk.writeUInt32BE(chunk.length, 4);
    payload.copy(chunk, 8);
    return chunk;
  });
  const totalLength = 8 + encodedChunks.reduce((total, chunk) => total + chunk.length, 0);
  const buffer = Buffer.alloc(totalLength);
  buffer.write("icns", 0, "ascii");
  buffer.writeUInt32BE(totalLength, 4);
  let offset = 8;
  for (const chunk of encodedChunks) {
    chunk.copy(buffer, offset);
    offset += chunk.length;
  }
  return buffer;
}

function replaceIcnsPayload(buffer, type, payload) {
  return makeIcns(readIcnsChunks(buffer).map((chunk) => chunk.type === type ? { type, payload } : chunk));
}

describe("assertRequiredIcoSizes", () => {
  test("rejects an ICO without a 24px layer", () => {
    expect(() => assertRequiredIcoSizes(makeIcoDirectory([16, 32, 48, 64, 256])))
      .toThrow("icon.ico is missing 24x24");
  });

  test("accepts every required Windows layer", () => {
    expect(() => assertRequiredIcoSizes(makeIcoDirectory([16, 24, 32, 48, 64, 256])))
      .not.toThrow();
  });

  test("rejects truncated directory entries and payloads", () => {
    const truncatedDirectory = makeIcoDirectory([16, 24, 32, 48, 64, 256]).subarray(0, 20);
    expect(() => assertRequiredIcoSizes(truncatedDirectory)).toThrow("icon.ico directory is truncated");

    const truncatedPayload = makeIcoDirectory([16, 24, 32, 48, 64, 256]);
    truncatedPayload.writeUInt32LE(truncatedPayload.length, 6 + 12);
    expect(() => assertRequiredIcoSizes(truncatedPayload)).toThrow("icon.ico entry 1 payload is out of bounds");
  });
});

describe("assertPng", () => {
  test("validates PNG dimensions and channel mode", async () => {
    const directory = await mkdtemp(join(tmpdir(), "markra-icon-verify-"));
    const rgbaPath = join(directory, "rgba.png");
    const rgbPath = join(directory, "rgb.png");
    await sharp({ create: { width: 32, height: 32, channels: 4, background: "#F9B52B" } }).png().toFile(rgbaPath);
    await sharp({ create: { width: 32, height: 32, channels: 3, background: "#F9B52B" } }).png().toFile(rgbPath);

    await expect(assertPng(rgbaPath, { width: 32, height: 32, channels: 4 })).resolves.toBeUndefined();
    await expect(assertPng(rgbPath, { width: 32, height: 32, channels: 4 })).rejects.toThrow("must use 4 channels");
    await expect(assertPng(rgbaPath, { width: 64, height: 64, channels: 4 })).rejects.toThrow("must be a 64x64 PNG");
  });
});

describe("assertValidIcns", () => {
  test("rejects malformed container lengths", async () => {
    const buffer = Buffer.alloc(8);
    buffer.write("icns", 0, "ascii");
    buffer.writeUInt32BE(16, 4);
    await expect(assertValidIcns(buffer, "test.icns")).rejects.toThrow("test.icns has an invalid container length");
  });

  test("rejects a missing representation", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));
    const withoutIc14 = makeIcns(readIcnsChunks(canonical).filter(({ type }) => type !== "ic14"));

    await expect(assertValidIcns(withoutIc14, "test.icns")).rejects.toThrow("test.icns is missing the ic14 representation");
  });

  test("rejects empty and corrupt PNG representations", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));

    await expect(assertValidIcns(replaceIcnsPayload(canonical, "ic10", Buffer.alloc(0)), "test.icns"))
      .rejects.toThrow("test.icns ic10 representation is empty");
    await expect(assertValidIcns(replaceIcnsPayload(canonical, "ic10", Buffer.from("not a png")), "test.icns"))
      .rejects.toThrow("test.icns ic10 must decode as a 1024x1024 PNG");
  });

  test("rejects a decoded PNG representation with the wrong dimensions", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));
    const wrongSize = await sharp({ create: { width: 8, height: 8, channels: 4, background: "#F9B52B" } }).png().toBuffer();

    await expect(assertValidIcns(replaceIcnsPayload(canonical, "ic10", wrongSize), "test.icns"))
      .rejects.toThrow("test.icns ic10 must decode as a 1024x1024 PNG");
  });

  test("rejects a decoded PNG representation without an alpha channel", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));
    const rgbOnly = await sharp({ create: { width: 32, height: 32, channels: 3, background: "#F9B52B" } }).png().toBuffer();

    await expect(assertValidIcns(replaceIcnsPayload(canonical, "ic11", rgbOnly), "test.icns"))
      .rejects.toThrow("test.icns ic11 must decode as a 32x32 RGBA PNG");
  });

  test("rejects corrupt ARGB/RLE representations", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));
    const corruptArgb = Buffer.concat([Buffer.from("ARGB"), Buffer.from([0])]);

    await expect(assertValidIcns(replaceIcnsPayload(canonical, "ic04", corruptArgb), "test.icns"))
      .rejects.toThrow("test.icns ic04 has invalid ARGB/RLE data for 16x16");
  });

  test("accepts and decodes every representation in the canonical icon", async () => {
    const canonical = await readFile(join(repoRoot, "assets/branding/app-icon/macos-icon.icns"));

    await expect(assertValidIcns(canonical, "test.icns")).resolves.toBeUndefined();
  });
});

test("rejects an hdpi foreground pixel below the rounded-up safe-area boundary", async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "markra-icon-safe-region-"));
  await mkdir(join(fixtureRoot, "apps/desktop/src-tauri"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/desktop/public"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/web/public"), { recursive: true });
  await mkdir(join(fixtureRoot, "assets/branding"), { recursive: true });
  await cp(join(repoRoot, "apps/desktop/src-tauri/icons"), join(fixtureRoot, "apps/desktop/src-tauri/icons"), { recursive: true });
  await cp(join(repoRoot, "assets/branding/app-icon"), join(fixtureRoot, "assets/branding/app-icon"), { recursive: true });
  await cp(join(repoRoot, "apps/desktop/public/favicon.png"), join(fixtureRoot, "apps/desktop/public/favicon.png"));
  await cp(join(repoRoot, "apps/web/public/favicon.png"), join(fixtureRoot, "apps/web/public/favicon.png"));

  const hdpiForeground = Buffer.alloc(162 * 162 * 4);
  hdpiForeground[(32 * 162 + 31) * 4 + 3] = 255;
  await sharp(hdpiForeground, { raw: { width: 162, height: 162, channels: 4 } })
    .png()
    .toFile(join(fixtureRoot, "apps/desktop/src-tauri/icons/android/mipmap-hdpi/ic_launcher_foreground.png"));

  await expect(verifyIconAssets(fixtureRoot)).rejects.toThrow("visible bounds must stay inside 32..130");
});

test("rejects an iOS icon that is not rendered from the full-bleed master", async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "markra-icon-ios-master-"));
  await mkdir(join(fixtureRoot, "apps/desktop/src-tauri"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/desktop/public"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/web/public"), { recursive: true });
  await mkdir(join(fixtureRoot, "assets/branding"), { recursive: true });
  await cp(join(repoRoot, "apps/desktop/src-tauri/icons"), join(fixtureRoot, "apps/desktop/src-tauri/icons"), { recursive: true });
  await cp(join(repoRoot, "assets/branding/app-icon"), join(fixtureRoot, "assets/branding/app-icon"), { recursive: true });
  await cp(join(repoRoot, "apps/desktop/public/favicon.png"), join(fixtureRoot, "apps/desktop/public/favicon.png"));
  await cp(join(repoRoot, "apps/web/public/favicon.png"), join(fixtureRoot, "apps/web/public/favicon.png"));

  await sharp({ create: { width: 20, height: 20, channels: 3, background: "#F9B52B" } })
    .png()
    .toFile(join(fixtureRoot, "apps/desktop/src-tauri/icons/ios/AppIcon-20x20@1x.png"));

  await expect(verifyIconAssets(fixtureRoot)).rejects.toThrow("must be rendered from the full-bleed iOS master");
});

test("all committed platform icon families pass verification", async () => {
  await expect(verifyIconAssets(repoRoot)).resolves.toBeUndefined();
});
