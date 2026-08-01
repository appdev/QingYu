export type RgbColor = readonly [number, number, number];

function parseRgbChannel(value: string) {
  const trimmed = value.trim();
  const parsed = trimmed.endsWith("%")
    ? Number.parseFloat(trimmed) * 2.55
    : Number.parseFloat(trimmed);

  if (!Number.isFinite(parsed) || parsed < 0 || parsed > 255) return null;
  return Math.round(parsed);
}

function parseAlpha(value: string | undefined) {
  if (value === undefined) return 1;

  const trimmed = value.trim();
  const parsed = trimmed.endsWith("%")
    ? Number.parseFloat(trimmed) / 100
    : Number.parseFloat(trimmed);

  return Number.isFinite(parsed) ? parsed : null;
}

export function parseComputedRgb(value: string): RgbColor | null {
  const normalized = value.trim();
  const srgbMatch = normalized.match(/^color\(srgb\s+(.+)\)$/iu);
  if (srgbMatch) {
    const [channelSource, alphaSource] = srgbMatch[1].split("/").map((part) => part.trim());
    const alpha = parseAlpha(alphaSource);
    const channels = channelSource.split(/\s+/u).filter(Boolean).map((channel) => {
      const parsed = channel.endsWith("%")
        ? Number.parseFloat(channel) / 100
        : Number.parseFloat(channel);
      return Number.isFinite(parsed) && parsed >= 0 && parsed <= 1
        ? Math.round(parsed * 255)
        : null;
    });

    if (channels.length !== 3 || channels.some((channel) => channel === null)) return null;
    if (alpha === null || alpha < 1) return null;
    return channels as [number, number, number];
  }

  const match = normalized.match(/^rgba?\((.*)\)$/iu);
  if (!match) return null;

  const body = match[1];
  const [channelSource, slashAlpha] = body.split("/").map((part) => part.trim());
  const commaParts = channelSource.split(",").map((part) => part.trim());
  const parts = commaParts.length > 1
    ? commaParts
    : channelSource.split(/\s+/u).filter(Boolean);
  const alpha = parseAlpha(slashAlpha ?? parts[3]);
  const channels = parts.slice(0, 3).map(parseRgbChannel);

  if (channels.length !== 3 || channels.some((channel) => channel === null)) return null;
  if (alpha === null || alpha < 1) return null;

  return channels as [number, number, number];
}

export function relativeLuminance(rgb: RgbColor) {
  const linear = rgb.map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });

  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

export function contrastRatio(left: RgbColor, right: RgbColor) {
  const lighter = Math.max(relativeLuminance(left), relativeLuminance(right));
  const darker = Math.min(relativeLuminance(left), relativeLuminance(right));
  return (lighter + 0.05) / (darker + 0.05);
}

function mixRgb(from: RgbColor, to: RgbColor, amount: number): RgbColor {
  return from.map((channel, index) => (
    Math.round(channel + (to[index] - channel) * amount)
  )) as [number, number, number];
}

export function fitContrast(
  background: RgbColor,
  preferred: RgbColor,
  targetRatio: number
): RgbColor {
  const black: RgbColor = [0, 0, 0];
  const white: RgbColor = [255, 255, 255];
  const destination = contrastRatio(preferred, background) >= targetRatio
    ? preferred
    : contrastRatio(black, background) > contrastRatio(white, background)
      ? black
      : white;

  for (let step = 1; step <= 100; step += 1) {
    const candidate = mixRgb(background, destination, step / 100);
    if (contrastRatio(candidate, background) >= targetRatio) return candidate;
  }

  return destination;
}

export function ensureContrast(
  background: RgbColor,
  preferred: RgbColor,
  targetRatio: number
) {
  return contrastRatio(preferred, background) >= targetRatio
    ? preferred
    : fitContrast(background, preferred, targetRatio);
}

export function rgbColorValue(rgb: RgbColor) {
  return `rgb(${rgb.join(" ")})`;
}
