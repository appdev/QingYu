export const preferencesKey = 'miaoyan-editor.preferences.v1';
export const sizes = [12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 24, 26, 28] as const;
export type Preferences = {
  version: 1;
  appearance: 'system' | 'light' | 'dark';
  buttonDisplay: 'always' | 'hover';
  alwaysOnTop: boolean;
  defaultMode: 'edit' | 'split' | 'preview' | 'rich';
  lineWrap: boolean;
  previewWidth: number;
  editorFont: string; editorFontSize: number;
  previewFont: string; previewFontSize: number;
  codeFont: string; interfaceFont: string;
};
export const defaults: Preferences = {
  version: 1, appearance: 'system', buttonDisplay: 'always', alwaysOnTop: false,
  defaultMode: 'edit', lineWrap: true, previewWidth: 1000,
  editorFont: 'system', editorFontSize: 16, previewFont: 'system', previewFontSize: 16,
  codeFont: 'text', interfaceFont: 'system',
};
export function parsePreferences(value: unknown): Preferences {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('设置数据无效');
  const p = { ...defaults, ...value } as Preferences;
  if (p.version !== 1 || !['system', 'light', 'dark'].includes(p.appearance)
    || !['always', 'hover'].includes(p.buttonDisplay) || typeof p.alwaysOnTop !== 'boolean'
    || !['edit', 'split', 'preview', 'rich'].includes(p.defaultMode) || typeof p.lineWrap !== 'boolean'
    || ![0, 600, 800, 1000, 1200, 1400].includes(p.previewWidth)
    || !sizes.includes(p.editorFontSize as typeof sizes[number]) || !sizes.includes(p.previewFontSize as typeof sizes[number])
    || [p.editorFont, p.previewFont, p.codeFont, p.interfaceFont].some(font => typeof font !== 'string' || !font.trim() || font.length > 200 || /[\x00-\x1f]/.test(font))) throw new Error('设置数据无效');
  // Keep only known fields; connection credentials never belong in preferences.
  return Object.fromEntries(Object.keys(defaults).map(key => [key, p[key as keyof Preferences]])) as Preferences;
}
type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;
export class PreferenceStore {
  value: Preferences = { ...defaults };
  error = '';
  private listeners = new Set<(p: Preferences) => void>();
  constructor(private storage: Storage) {
    try { const raw = storage.getItem(preferencesKey); if (raw) this.value = parsePreferences(JSON.parse(raw)); }
    catch { this.error = '无法读取已保存的设置；当前使用默认值。可恢复默认设置后重试。'; }
  }
  subscribe(fn: (p: Preferences) => void) { this.listeners.add(fn); return () => { this.listeners.delete(fn); }; }
  update(change: Partial<Preferences>, reset = false) {
    if (this.error && !reset) throw new Error(this.error);
    const next = parsePreferences(reset ? defaults : { ...this.value, ...change });
    this.storage.setItem(preferencesKey, JSON.stringify(next));
    this.error = ''; this.receive(next);
  }
  receive(value: unknown) {
    const next = parsePreferences(value);
    if (JSON.stringify(next) === JSON.stringify(this.value)) return;
    this.value = next; for (const fn of this.listeners) fn(next);
  }
}
export const preferences = new PreferenceStore(localStorage);
export const appearance = matchMedia('(prefers-color-scheme: dark)');
export function isDark() { return preferences.value.appearance === 'dark' || preferences.value.appearance === 'system' && appearance.matches; }
const systemFont = "ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', sans-serif";
export function fontCSS(font: string) { return font === 'system' ? systemFont : `${JSON.stringify(font)}, ${systemFont}`; }
export function previewPreferences() {
  const p = preferences.value;
  return { dark: isDark(), font: fontCSS(p.previewFont), codeFont: fontCSS(p.codeFont === 'text' ? p.previewFont : p.codeFont), size: p.previewFontSize, width: p.previewWidth };
}
function apply() {
  const p = preferences.value, root = document.documentElement, preview = previewPreferences();
  root.dataset.theme = isDark() ? 'dark' : 'light';
  root.dataset.buttonDisplay = p.buttonDisplay;
  root.classList.toggle('darkmode', isDark()); root.style.colorScheme = root.dataset.theme;
  for (const [key, value] of Object.entries({ '--font': fontCSS(p.interfaceFont), '--editor-font': fontCSS(p.editorFont), '--editor-size': `${p.editorFontSize}px`, '--editor-line-height': `${p.editorFontSize * 1.675}px`, '--preview-font': preview.font, '--code-font': preview.codeFont, '--preview-size': `${preview.size}px`, '--preview-width': p.previewWidth ? `${p.previewWidth}px` : '100%' })) root.style.setProperty(key, value);
  document.body?.classList.toggle('darkmode', isDark());
  window.nativeAppearance?.postMessage(root.dataset.theme);
  window.dispatchEvent(new Event('preferences-applied'));
}
preferences.subscribe(apply); appearance.addEventListener('change', apply); apply();
window.addEventListener('storage', event => {
  if (event.key !== preferencesKey || !event.newValue) return;
  try { preferences.receive(JSON.parse(event.newValue)); } catch { /* Keep the last valid live settings. */ }
});
export const native = '__TAURI_INTERNALS__' in window;
export const mobile = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent) || navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1;
export function settingsShortcut(event: KeyboardEvent, mac = /Mac/.test(navigator.platform)) {
  return event.key === ',' && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) && !event.altKey && !event.shiftKey && !event.isComposing;
}
