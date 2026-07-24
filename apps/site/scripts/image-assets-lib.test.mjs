import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import sharp from "sharp";
import { requireImageDimensions } from "./image-assets-lib.mjs";

test("accepts an image with the required dimensions", async () => {
  const directory = await mkdtemp(join(tmpdir(), "qingyu-site-icon-"));
  const imagePath = join(directory, "icon.png");

  try {
    await sharp({
      create: { width: 64, height: 64, channels: 4, background: "transparent" }
    }).png().toFile(imagePath);

    await assert.doesNotReject(requireImageDimensions(imagePath, 64, 64));
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("rejects an image with unexpected dimensions", async () => {
  const directory = await mkdtemp(join(tmpdir(), "qingyu-site-icon-"));
  const imagePath = join(directory, "icon.png");

  try {
    await sharp({
      create: { width: 32, height: 32, channels: 4, background: "transparent" }
    }).png().toFile(imagePath);

    await assert.rejects(
      requireImageDimensions(imagePath, 64, 64),
      /Expected .*icon\.png to be 64x64, received 32x32\./u
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
