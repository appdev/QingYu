export type DebugLogFactory = () => readonly [message: string, details?: unknown];

declare const __MARKRA_DEBUG__: boolean;

export function debug(factory: DebugLogFactory) {
  if (typeof __MARKRA_DEBUG__ === "undefined" || !__MARKRA_DEBUG__) return;

  const [message, details] = factory();
  if (details === undefined) {
    console.debug(message);
    return;
  }

  console.debug(message, details);
}
