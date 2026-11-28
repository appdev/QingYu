import { bindLabels, translatedLabel } from './ui-language';
import './style.css';
import { listen } from '@tauri-apps/api/event';
import { connectPreferences, remoteSync } from './settings-bridge';
import { connectEditorHistoryMenu } from './editor-history-menu';
import { mountSettings, type SettingsSection } from './settings-view';
import { mountFeatureWindow } from './feature-panel';

const root = document.querySelector<HTMLElement>('#settings-root')!;
const localizeLabels = bindLabels(document);
localizeLabels();
window.addEventListener('ui-language-change', localizeLabels);
document.addEventListener('keydown', event => {
  if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'a'
    && !(document.activeElement instanceof Element && document.activeElement.closest('input, textarea, [contenteditable]:not([contenteditable=false]), [data-selectable]'))) {
    event.preventDefault();
  }
});
void connectEditorHistoryMenu().catch(console.warn);
await mountFeatureWindow(root, async close => {
  let view: ReturnType<typeof mountSettings> | undefined;
  let unlisten: (() => void) | undefined;
  try {
    await connectPreferences();
    view = mountSettings(root, await remoteSync(), close);
    view.select(new URLSearchParams(location.search).get('section') as SettingsSection ?? 'general');
    unlisten = await listen<SettingsSection>('settings-section', ({ payload }) => view?.select(payload));
  } catch { root.textContent = translatedLabel('无法打开设置，请关闭窗口后重试。'); }
  return () => { unlisten?.(); view?.destroy(); };
});
