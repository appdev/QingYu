import { invoke } from '@tauri-apps/api/core';
import { native, mobile } from './preferences';
import { mountSettings, type SettingsSection } from './settings-view';
import type { SyncClient } from './sync-session';

let inline: ReturnType<typeof mountSettings> | undefined;
export async function openSettings(client: SyncClient, section: SettingsSection = 'general') {
  if (native && !mobile) { await invoke('open_settings', { section }); return; }
  if (inline) { inline.select(section); return; }
  const root = document.createElement('dialog'); root.className = 'settings-inline'; root.setAttribute('aria-label', '设置');
  const previous = document.activeElement as HTMLElement | null;
  history.pushState({ settings: true }, '');
  function close() { history.back(); }
  function remove() { inline?.destroy(); inline = undefined; root.remove(); window.removeEventListener('popstate', navigate); previous?.focus(); }
  function navigate(event: PopStateEvent) { if (!event.state?.settings) remove(); }
  root.addEventListener('cancel', event => { event.preventDefault(); close(); });
  root.addEventListener('keydown', event => event.stopPropagation());
  window.addEventListener('popstate', navigate);
  document.body.append(root); inline = mountSettings(root, client, close); inline.select(section); root.showModal(); root.querySelector<HTMLButtonElement>('.settings-back')!.focus();
}
