export function normalizeNullableString(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}
