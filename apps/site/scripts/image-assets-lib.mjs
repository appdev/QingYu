import sharp from "sharp";

export async function requireImageDimensions(path, width, height) {
  const metadata = await sharp(path).metadata();
  if (metadata.width !== width || metadata.height !== height) {
    throw new Error(
      `Expected ${path} to be ${width}x${height}, received ${metadata.width}x${metadata.height}.`
    );
  }
}
