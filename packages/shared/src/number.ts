export function clampNumber(value: unknown, min: number, max: number) {
  if (typeof value !== "number" || Number.isNaN(value)) return null;

  return Math.max(min, Math.min(max, value));
}
