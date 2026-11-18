import './style.css';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { connectPreferences, remoteSync } from './settings-bridge';
import { connectEditorHistoryMenu } from './editor-history-menu';
import { mountSettings, type SettingsSection } from './settings-view';
import { showWindowWhenPainted } from './window-ready';

const root = document.querySelector<HTMLElement>('#settings-root')!;
root.style.height = '100%';
void connectEditorHistoryMenu().catch(console.warn);
try {
  await connectPreferences();
  const view = mountSettings(root, await remoteSync(), () => { void getCurrentWindow().close(); });
  view.select(new URLSearchParams(location.search).get('section') as SettingsSection ?? 'general');
  await listen<SettingsSection>('settings-section', ({ payload }) => view.select(payload));
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'Escape' || event.key.toLowerCase() === 'w' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); void getCurrentWindow().close(); }
  });
} catch { root.textContent = '无法打开设置，请关闭窗口后重试。'; }
showWindowWhenPainted();
