import { invoke } from '@tauri-apps/api/core';
import { native, mobile } from './preferences';
import { mountSettings, type SettingsSection } from './settings-view';
import type { SyncClient } from './sync-session';
import type { DailyNoteChoices } from './daily-note';

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
export async function openSettings(client: SyncClient, section?: SettingsSection, loadDaily?: () => DailyNoteChoices) {
  if (native && !mobile) { await invoke('open_settings', { section: section ?? 'general' }); return; }
  if (inline) { if (section) showSection(section); return; }
  const root = document.createElement('dialog'); root.className = 'settings-inline'; root.setAttribute('aria-label', '设置');
  const previous = document.activeElement as HTMLElement | null;
  // A reload can retain a settings history entry after its dialog has gone.
  const base = { ...history.state };
  delete base.settings; delete base.settingsSection; delete base.settingsDepth;
  if (history.state?.settings) history.replaceState(base, '');
  history.pushState({ ...base, settings: true }, '');
  let returning = false;
  function close() { if (!returning && !root.inert) { returning = true; history.back(); } }
  function remove() { inline?.destroy(); inline = undefined; root.remove(); window.removeEventListener('popstate', navigate); previous?.focus({ preventScroll: true }); }
  function navigate(event: PopStateEvent) {
    if (root.inert) return;
    returning = false;
    if (!event.state?.settings) {
      if (!mobile) { remove(); return; }
      root.inert = true;
      root.dataset.settingsClosing = '';
      const animations = root.getAnimations?.() ?? [];
      if (animations.length) void Promise.allSettled(animations.map(animation => animation.finished)).then(remove);
      else remove();
    }
    else if (mobile) inline?.select(event.state.settingsSection, true);
  }
  if (mobile) {
    function dismiss() {
      if (returning || root.inert) return;
      returning = true;
      history.go(-1 - (history.state?.settingsDepth ?? 0));
    }
    root.addEventListener('click', event => {
      if (event.target !== root) return;
      const { left, right, top, bottom } = root.getBoundingClientRect();
      if (event.clientX < left || event.clientX > right || event.clientY < top || event.clientY > bottom) dismiss();
    });

    let drag: { pointerId: number; startY: number; height: number } | undefined;
    function move(clientY: number, startY: number, height: number) {
      const offset = Math.min(height, Math.max(0, clientY - startY));
      root.style.setProperty('--settings-sheet-transform', `translateY(${offset}px)`);
      root.style.setProperty('--settings-backdrop-opacity', String(1 - offset / height));
      return offset;
    }
    function endDrag(dismissed = false) {
      drag = undefined;
      root.removeAttribute('data-settings-dragging');
      if (dismissed) { dismiss(); return; }
      root.style.removeProperty('--settings-sheet-transform');
      root.style.removeProperty('--settings-backdrop-opacity');
    }
    root.addEventListener('pointerdown', event => {
      const header = (event.target as HTMLElement).closest<HTMLElement>('.settings-header');
      if (!header || (event.target as HTMLElement).closest('button') || !event.isPrimary || event.button !== 0 || drag || returning || root.inert) return;
      if (root.getAnimations?.().some(animation => animation.playState === 'running')) return;
      const height = root.getBoundingClientRect().height;
      if (!height) return;
      drag = { pointerId: event.pointerId, startY: event.clientY, height };
      root.dataset.settingsDragging = '';
      header.setPointerCapture?.(event.pointerId);
    });
    root.addEventListener('pointermove', event => {
      if (drag?.pointerId === event.pointerId) move(event.clientY, drag.startY, drag.height);
    });
    root.addEventListener('pointerup', event => {
      if (drag?.pointerId !== event.pointerId) return;
      const offset = move(event.clientY, drag.startY, drag.height);
      endDrag(offset >= Math.min(120, drag.height / 4));
    });
    for (const type of ['pointercancel', 'lostpointercapture'] as const) root.addEventListener(type, event => {
      if (drag?.pointerId === event.pointerId) endDrag();
    });
  }
  root.addEventListener('cancel', event => { event.preventDefault(); close(); });
  root.addEventListener('keydown', event => event.stopPropagation());
  window.addEventListener('popstate', navigate);
  document.body.append(root);
  inline = mountSettings(root, client, close, showSection, loadDaily);
  root.showModal();
  root.querySelector<HTMLElement>(mobile ? '.settings-title' : '.settings-close')!.focus({ preventScroll: true });
  if (section) showSection(section);
}
