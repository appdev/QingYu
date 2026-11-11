type Response = { status: number; headers: Record<string, string>; arrayBuffer: ArrayBuffer; text: string; json: unknown };
type Request = { url: string; method: string; headers?: Record<string, string>; body?: ArrayBuffer };
let handler: ((request: Request) => Promise<Response>) | undefined;
export function setRequestHandler(next: typeof handler) { handler = next; }
export function requestUrl(request: Request) {
  if (!handler) throw new Error('S3 网络尚未配置');
  return handler(request);
}
export function normalizePath(path: string) { return path.replaceAll('\\', '/').replace(/\/{2,}/g, '/').replace(/^\/+|\/+$/g, ''); }
export const Platform = { isMobile: false, isDesktop: false, isMacOS: false, isWin: false };
export class Notice { constructor(_message: unknown) { /* The application owns status/error presentation. */ } }
// UI-only facade exports are removed by tree shaking; any accidental runtime use fails closed.
class UnsupportedHost { constructor() { throw new Error('Obsidian UI is unavailable'); } }
export { UnsupportedHost as ButtonComponent, UnsupportedHost as Modal, UnsupportedHost as Setting,
  UnsupportedHost as SettingPage, UnsupportedHost as FileSystemAdapter };
export function setIcon() { throw new Error('Obsidian UI is unavailable'); }
export function debounce() { throw new Error('Obsidian UI is unavailable'); }
