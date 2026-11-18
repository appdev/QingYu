import { themeNames, themePalette, type Theme } from './themes';
import { dailyTitleStyles, type DailyNoteSettings } from './daily-note';

export const preferencesKey = 'miaoyan-editor.preferences.v1';
export const mobile = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent) || navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1;
export const sizes = [12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 24, 26, 28] as const;
export const noteTextSizes = [13, 15, 17, 20, 24] as const;
export const syncIntervals = [1, 3, 5, 10, 15, 30, 60] as const;
export type Preferences = DailyNoteSettings & {
  version: 1;
  appearance: 'system' | 'light' | 'dark';
  theme: Theme;
  buttonDisplay: 'always' | 'hover';
  alwaysOnTop: boolean;
  /** 桌面上在菜单栏（Windows 通知区域、Linux 托盘）显示轻羽，和 Pinto 一样默认打开。 */
  menuBar: boolean;
  sortCheckedItems: boolean;
  todayOpenOnLaunch: boolean;
  previewWidth: number;
  previewFont: string; previewFontSize: number;
  syncInterval: typeof syncIntervals[number];
  autoSyncS3: boolean;
  autoSyncCloudAccount: string;
  cloudSyncAccount: string;
};
export const defaults: Preferences = {
  version: 1, appearance: 'system', theme: 'default', buttonDisplay: 'always', alwaysOnTop: false, menuBar: true,
  sortCheckedItems: true, previewWidth: 1000,
  todayFolder: '', todayTitle: 'long', todayOpenOnLaunch: false,
  previewFont: 'system', previewFontSize: mobile ? 17 : noteTextSizes[0],
  syncInterval: 3, autoSyncS3: true, autoSyncCloudAccount: '', cloudSyncAccount: '',
};
export function parsePreferences(value: unknown): Preferences {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('设置数据无效');
  const p = { ...defaults, ...value };
  if (p.version !== 1 || !['system', 'light', 'dark'].includes(p.appearance)
    || typeof p.theme !== 'string' || !Object.hasOwn(themeNames, p.theme)
    || !['always', 'hover'].includes(p.buttonDisplay) || typeof p.alwaysOnTop !== 'boolean' || typeof p.menuBar !== 'boolean'
    || typeof p.sortCheckedItems !== 'boolean'
    || !dailyTitleStyles.includes(p.todayTitle) || typeof p.todayOpenOnLaunch !== 'boolean'
    || typeof p.todayFolder !== 'string' || p.todayFolder.length > 500 || /[\x00-\x1f]/.test(p.todayFolder)
    || ![0, 600, 800, 1000, 1200, 1400].includes(p.previewWidth)
    || !syncIntervals.includes(p.syncInterval) || typeof p.autoSyncS3 !== 'boolean'
    || typeof p.autoSyncCloudAccount !== 'string' || p.autoSyncCloudAccount.length > 512 || /[\x00-\x1f]/.test(p.autoSyncCloudAccount)
    || typeof p.cloudSyncAccount !== 'string' || p.cloudSyncAccount.length > 512 || /[\x00-\x1f]/.test(p.cloudSyncAccount)
    || !sizes.includes(p.previewFontSize as typeof sizes[number])
    || typeof p.previewFont !== 'string' || !p.previewFont.trim() || p.previewFont.length > 200 || /[\x00-\x1f]/.test(p.previewFont)) throw new Error('设置数据无效');
  if (!mobile) p.previewFontSize = noteTextSizes.find(size => size >= p.previewFontSize) ?? noteTextSizes.at(-1)!;
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
    catch { this.error = '无法读取已保存的设置，当前使用默认值。'; }
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
export function changeNoteTextSize(action: 'bigger' | 'smaller' | 'actual') {
  const current = preferences.value.previewFontSize;
  const index = noteTextSizes.findIndex(size => size === current);
  const next = action === 'actual' ? noteTextSizes[0] : noteTextSizes[index + (action === 'bigger' ? 1 : -1)];
  if (next !== undefined && next !== current) preferences.update({ previewFontSize: next });
}
export function noteTextSizeShortcut(event: KeyboardEvent) {
  if (event.isComposing || event.altKey || !(event.metaKey || event.ctrlKey)) return;
  if (event.key === '+' || event.key === '=') return 'bigger';
  if (event.shiftKey) return;
  if (event.key === '-') return 'smaller';
  if (event.key === '0') return 'actual';
}
export const appearance = matchMedia('(prefers-color-scheme: dark)');
export function isDark() { return preferences.value.appearance === 'dark' || preferences.value.appearance === 'system' && appearance.matches; }
const systemFont = "ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', sans-serif";
export function fontCSS(font: string) {
  if (font === 'system') return systemFont;
  if (font === 'ui-serif') return `ui-serif, "New York", Georgia, "Songti SC", serif`;
  if (font === 'ui-monospace') return `ui-monospace, "SF Mono", Menlo, monospace`;
  if (font === 'ui-rounded') return `ui-rounded, "Arial Rounded MT Bold", ${systemFont}`;
  return `${JSON.stringify(font)}, ${systemFont}`;
}
export function previewPreferences() {
  const p = preferences.value;
  return { dark: isDark(), theme: p.theme, font: fontCSS(p.previewFont), size: p.previewFontSize, width: p.previewWidth };
}
const appShell = document.querySelector<HTMLElement>('#app-shell');
function updateNativeAppearance() {
  if (!window.nativeAppearance) return;
  const dark = isDark(), colors = themePalette(preferences.value.theme, dark);
  const page = appShell?.hasAttribute('data-mobile-wide') ? undefined : appShell?.dataset.mobilePage;
  const background = page === 'folders' ? colors['mobile-sidebar'] : page === 'notes' ? colors['mobile-list'] : colors.paper;
  window.nativeAppearance.postMessage(`${dark ? 'dark' : 'light'}:${background}`);
}
if (window.nativeAppearance && appShell) {
  new MutationObserver(updateNativeAppearance).observe(appShell, { attributes: true, attributeFilter: ['data-mobile-page', 'data-mobile-wide'] });
}
function apply() {
  const p = preferences.value, root = document.documentElement, preview = previewPreferences();
  const theme = preview.dark ? 'dark' : 'light';
  root.dataset.theme = theme;
  const colors = themePalette(p.theme, preview.dark);
  for (const [key, value] of Object.entries(colors)) {
    if (p.theme === 'default') root.style.removeProperty(`--${key}`);
    else root.style.setProperty(`--${key}`, value);
  }
  Object.assign(window, { QingYuPalette: { light: themePalette(p.theme, false), dark: themePalette(p.theme, true) } });
  root.toggleAttribute('data-mobile', mobile);
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', colors.paper);
  root.dataset.buttonDisplay = p.buttonDisplay;
  root.classList.toggle('darkmode', preview.dark); root.style.colorScheme = theme;
  for (const [key, value] of Object.entries({ '--preview-font': preview.font, '--preview-size': `${preview.size}px`, '--preview-width': p.previewWidth ? `${p.previewWidth}px` : '100%' })) root.style.setProperty(key, value);
  document.body?.classList.toggle('darkmode', preview.dark);
  updateNativeAppearance();
  window.dispatchEvent(new Event('preferences-applied'));
}
preferences.subscribe(apply); appearance.addEventListener('change', apply); apply();
window.addEventListener('storage', event => {
  if (event.key !== preferencesKey || !event.newValue) return;
  try { preferences.receive(JSON.parse(event.newValue)); } catch { /* Keep the last valid live settings. */ }
});
export const native = '__TAURI_INTERNALS__' in window;
export function settingsShortcut(event: KeyboardEvent, mac = /Mac/.test(navigator.platform)) {
  return event.key === ',' && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) && !event.altKey && !event.shiftKey && !event.isComposing;
}
