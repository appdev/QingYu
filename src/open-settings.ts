import { invoke } from '@tauri-apps/api/core';
import { mobile } from './preferences';
import { openFeaturePanel } from './feature-panel';
import { mountSettings, type SettingsSection } from './settings-view';
import type { SyncClient } from './sync-session';
import type { DailyNoteChoices } from './daily-note';
import { uiText } from './ui-language';

let inline: ReturnType<typeof mountSettings> | undefined;
function showSection(section: SettingsSection) {
  const selected = inline?.select(section, mobile);
  if (!mobile || !selected || history.state?.settingsSection === selected) return;
  const nested = history.state?.settingsSection === 'account'
    && (selected === 'sync' || selected === 'storage' || selected === 'personal' || selected === 'signIn');
  const state = { ...history.state, settingsSection: selected, settingsDepth: nested ? (history.state?.settingsDepth ?? 0) + 1 : 1 };
  if (history.state?.settingsSection && !nested) history.replaceState(state, '');
  else history.pushState(state, '');
}
export async function openSettings(client: SyncClient | undefined, section?: SettingsSection, loadDaily?: () => DailyNoteChoices) {
  if (inline) { if (section) showSection(section); return; }
  await openFeaturePanel({
    title: uiText('settings.title'),
    className: 'settings-inline',
    animated: mobile,
    dragHandle: mobile ? '.settings-header' : undefined,
    openWindow: () => invoke('open_settings', { section: section ?? 'general' }),
    mount(root, close) {
      // A reload can retain a settings history entry after its dialog has gone.
      const base = { ...history.state };
      delete base.settings; delete base.settingsSection; delete base.settingsDepth;
      if (history.state?.settings) history.replaceState(base, '');
      history.pushState({ ...base, settings: true }, '');
      let returning = false;
      function back() {
        if (!returning && !root.inert) { returning = true; history.back(); }
      }
      function dismiss() {
        if (returning || root.inert) return;
        returning = true;
        history.go(-1 - (history.state?.settingsDepth ?? 0));
      }
      function navigate(event: PopStateEvent) {
        if (root.inert) return;
        returning = false;
        if (!event.state?.settings) close();
        else if (mobile) inline?.select(event.state.settingsSection, true);
      }
      inline = mountSettings(root, client, back, showSection, loadDaily);
      window.addEventListener('popstate', navigate);
      return {
        back,
        dismiss,
        focus: root.querySelector<HTMLElement>(mobile ? '.settings-title' : '.settings-close'),
        dispose() {
          window.removeEventListener('popstate', navigate);
          inline?.destroy();
          inline = undefined;
        },
      };
    },
  });
  if (section) showSection(section);
}
