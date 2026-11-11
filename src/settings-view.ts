import { invoke } from '@tauri-apps/api/core';
import { homeDir } from '@tauri-apps/api/path';
import { libraryLocation } from './library-settings';
import { createShortcutRecorder } from './shortcut-recorder';
import { settingsSelect } from './settings-select';
import { mobile, native, preferences, sizes, type Preferences } from './preferences';
import type { SyncClient } from './sync-session';
import type { S3Settings } from './sync-store';
import './settings.css';

export type SettingsSection = 'general' | 'editor' | 'typography' | 'sync';
const sections: [SettingsSection, string][] = [['general', '通用'], ['editor', '编辑器'], ['typography', '字体'], ['sync', '同步']];
export function mountSettings(root: HTMLElement, sync: SyncClient, close: () => void) {
  const visibleSections = sections.filter(([key]) => !mobile || key !== 'typography');
  root.classList.add('settings');
  root.classList.toggle('settings-mobile', mobile);
  root.innerHTML = `<header class="settings-header" data-tauri-drag-region>
    <button class="settings-back" aria-label="返回笔记"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 5-7 7 7 7"/></svg></button>
    <nav class="settings-tabs" role="tablist" aria-label="设置分类"></nav>
  </header><main class="settings-pages"></main>
  <footer class="settings-footer"><button class="settings-reset">恢复默认设置</button><span>设置即时生效</span></footer>
  <p class="settings-message" role="status" aria-live="polite"></p>`;
  const message = root.querySelector<HTMLElement>('.settings-message')!;
  function error(text: string) { message.textContent = text; message.dataset.error = 'true'; }
  root.querySelector<HTMLButtonElement>('.settings-back')!.onclick = close;
  const controls = new Map<keyof Preferences, HTMLInputElement | HTMLSelectElement>();
  const choices: ReturnType<typeof settingsSelect>[] = [];
  const pages = new Map<SettingsSection, HTMLElement>();
  const tabs: HTMLButtonElement[] = [];
  let selected: SettingsSection = 'general';
  function select(section: SettingsSection, focus = false) {
    if (!pages.has(section)) section = 'general';
    selected = section;
    for (const [key, page] of pages) { page.hidden = key !== section; page.setAttribute('aria-hidden', String(key !== section)); }
    root.querySelector('main')!.scrollTop = 0;
    for (const tab of tabs) { const active = tab.dataset.section === section; tab.setAttribute('aria-selected', String(active)); tab.tabIndex = active ? 0 : -1; if (active && focus) tab.focus(); }
  }
  for (const [key, title] of visibleSections) {
    const tab = document.createElement('button'); tab.type = 'button'; tab.id = `settings-tab-${key}`;
    tab.textContent = title; tab.dataset.section = key; tab.setAttribute('role', 'tab'); tab.setAttribute('aria-controls', `settings-panel-${key}`);
    tab.onclick = () => select(key); tabs.push(tab); root.querySelector('nav')!.append(tab);
    const page = document.createElement('section'); page.id = `settings-panel-${key}`; page.className = 'settings-page'; page.setAttribute('role', 'tabpanel'); page.setAttribute('aria-labelledby', tab.id); page.tabIndex = 0;
    pages.set(key, page); root.querySelector('main')!.append(page);
  }
  root.querySelector('nav')!.addEventListener('keydown', event => {
    const e = event as KeyboardEvent, index = visibleSections.findIndex(([key]) => key === selected), count = visibleSections.length;
    const next = e.key === 'ArrowRight' ? (index + 1) % count : e.key === 'ArrowLeft' ? (index + count - 1) % count : e.key === 'Home' ? 0 : e.key === 'End' ? count - 1 : -1;
    if (next >= 0) { e.preventDefault(); select(visibleSections[next][0], true); }
  });
  let rowId = 0;
  function row(section: SettingsSection, title: string, detail: string, control: HTMLElement, group = false) {
    const row = document.createElement('div'); row.className = `settings-row${group ? ' settings-group' : ''}`;
    const label = document.createElement('label'); label.className = 'settings-label';
    const heading = document.createElement('span'); heading.textContent = title;
    const description = document.createElement('small'); description.textContent = detail; description.id = `settings-detail-${section}-${rowId++}`;
    label.append(heading, description);
    const input = control.matches('input, select') ? control : control.querySelector('input, select');
    if (input) { input.id ||= `${description.id}-control`; label.htmlFor = input.id; input.setAttribute('aria-describedby', description.id); }
    row.append(label, control); pages.get(section)!.append(row); return row;
  }
  function setting(section: SettingsSection, key: keyof Preferences, title: string, detail: string, options?: [string | number, string][], group = false) {
    const control = options ? document.createElement('select') : document.createElement('input');
    control.className = 'settings-control'; control.name = key;
    if (control instanceof HTMLSelectElement) for (const [value, text] of options!) control.add(new Option(text, String(value)));
    else { control.type = 'checkbox'; control.setAttribute('role', 'switch'); }
    controls.set(key, control);
    control.onchange = () => {
      const value = control instanceof HTMLInputElement ? control.checked : typeof preferences.value[key] === 'number' ? Number(control.value) : control.value;
      try { preferences.update({ [key]: value }); message.textContent = ''; }
      catch (e) { error(e instanceof Error ? e.message : '无法保存设置'); updateControls(); }
    };
    const element = row(section, title, detail, control, group);
    if (mobile && control instanceof HTMLSelectElement) {
      const choice = settingsSelect(control, title); choices.push(choice); element.append(choice.element);
      element.querySelector('label')!.htmlFor = choice.button.id;
    }
    return control;
  }
  let recorder: ReturnType<typeof createShortcutRecorder> | undefined;
  let locationListener: (() => void) | undefined;
  if (native && !mobile) {
    const button = document.createElement('button'); button.type = 'button'; button.className = 'settings-control'; button.textContent = '更改'; button.setAttribute('aria-label', '更改笔记位置'); button.disabled = true;
    const element = row('general', '笔记位置', '正在读取…', button);
    const detail = element.querySelector('small')!;
    let current = '', home = '', changing = false;
    function display(path: string) {
      current = path; const base = home.replace(/[\\/]+$/, '');
      const shown = base && (path === base || path.startsWith(base + '/') || path.startsWith(base + '\\')) ? '~' + path.slice(base.length) : path || '尚未打开笔记文件夹';
      const split = Math.max(0, shown.lastIndexOf('/'), shown.lastIndexOf('\\'));
      const prefix = document.createElement('span'), tail = document.createElement('span');
      prefix.textContent = shown.slice(0, split); tail.textContent = shown.slice(split);
      detail.className = 'settings-path'; detail.replaceChildren(prefix, tail); detail.title = path;
    }
    const refreshLocation = async () => {
      try { [current, home] = await Promise.all([libraryLocation(), homeDir().catch(() => '')]); display(current); }
      catch (e) { detail.textContent = '无法读取笔记位置'; error(e instanceof Error ? e.message : '无法读取笔记位置'); }
      finally { button.disabled = false; }
    };
    button.onclick = async () => {
      if (changing) return;
      changing = true; button.disabled = true;
      try { const previous = current, path = await libraryLocation(true); display(path); message.dataset.error = 'false'; message.textContent = path !== previous ? '已切换笔记位置' : ''; }
      catch (e) { error(e instanceof Error ? e.message : '无法更改笔记位置'); }
      finally { changing = false; button.disabled = false; }
    };
    locationListener = () => { if (!changing) void refreshLocation(); };
    window.addEventListener('focus', locationListener); void refreshLocation();
  }
  setting('general', 'appearance', '外观', '选择浅色、深色或跟随系统', [['system', '跟随系统'], ['light', '浅色'], ['dark', '深色']], true);
  setting('general', 'buttonDisplay', '按钮显示', '控制编辑区顶部工具按钮的显示方式', [['always', '始终显示'], ['hover', '悬停时显示']]);
  if (!mobile && native) setting('general', 'alwaysOnTop', '窗口置顶', '让轻羽窗口保持在其他窗口上方', undefined, true);
  if (native && !mobile) {
    const note = document.createElement('p'); note.className = 'settings-shortcut-status'; note.dataset.error = 'true'; note.setAttribute('role', 'alert');
    recorder = createShortcutRecorder(text => { note.textContent = text; });
    row('general', '唤起快捷键', '在任何应用里呼出轻羽', recorder.control);
    pages.get('general')!.append(note);
  }
  setting('editor', 'defaultMode', '默认模式', '下次打开应用时使用的编辑模式', [['edit', '编辑'], ['split', '分屏预览'], ['preview', '预览'], ['rich', '所见即所得']]);
  setting('editor', 'lineWrap', '自动换行', '原文编辑时根据窗口宽度换行', undefined, true);
  setting('editor', 'previewWidth', '预览宽度', '预览与所见即所得使用相同正文宽度', [600, 800, 1000, 1200, 1400, 0].map(width => [width, width ? `${width} px` : '铺满窗口']));
  const fontControls: HTMLSelectElement[] = [];
  if (!mobile) for (const [key, title, detail] of [
    ['editorFont', '编辑字体', 'Markdown 原文的字体'], ['previewFont', '预览字体', '预览与所见即所得正文的字体'],
    ['codeFont', '代码字体', '预览与所见即所得代码块的字体'], ['interfaceFont', '界面字体', '笔记列表和侧栏的字体'],
  ] as const) {
    const select = setting('typography', key, title, detail, [[key === 'codeFont' ? 'text' : 'system', key === 'codeFont' ? '随正文' : '系统字体']]) as HTMLSelectElement;
    fontControls.push(select);
    if (key === 'editorFont' || key === 'previewFont') {
      const size = setting('typography', key === 'editorFont' ? 'editorFontSize' : 'previewFontSize', `${title.slice(0, 2)}字号`, '单位：像素', sizes.map(size => [size, `${size}`]));
      // MiaoYan places font family and size on the same row.
      const sizeRow = size.parentElement!; const fontRow = select.parentElement!;
      const pair = document.createElement('div'); pair.className = 'settings-font-pair'; select.replaceWith(pair); pair.append(select, size); size.setAttribute('aria-label', `${title.slice(0, 2)}字号`); size.removeAttribute('aria-describedby'); sizeRow.remove(); fontRow.querySelector('label')!.htmlFor = select.id;
    }
  }
  async function loadFonts() {
    let fonts = ['Arial', 'Georgia', 'Helvetica Neue', 'Menlo', 'Microsoft YaHei', 'Noto Sans', 'PingFang SC', 'SF Pro Text', 'Times New Roman'];
    if (native && !mobile) {
      try { fonts = await invoke<string[]>('system_fonts'); } catch { error('无法读取系统字体，已显示常用字体'); }
    }
    for (const control of fontControls) {
      const stored = preferences.value[control.name as 'editorFont'];
      for (const font of new Set([...fonts, stored])) if (![...control.options].some(option => option.value === font)) control.add(new Option(font, font));
    }
    updateControls();
  }
  const form = document.createElement('form'); form.className = 'settings-sync-form'; form.noValidate = true; form.setAttribute('aria-label', 'S3 同步'); pages.get('sync')!.append(form);
  for (const [key, title, detail] of [
    ['endpoint', '服务地址', '支持 S3 协议的 HTTPS 地址'], ['bucket', 'Bucket', '存储桶名称'], ['region', '区域', '默认 us-east-1'],
    ['prefix', '同步前缀', '不同笔记库请使用不同前缀'], ['accessKeyId', 'Access Key ID', 'S3 访问标识'], ['secretAccessKey', 'Secret Access Key', 'S3 访问密钥'],
  ]) {
    const input = document.createElement('input'); input.name = key; input.className = 'settings-control'; input.required = true; input.maxLength = 4096; input.autocomplete = 'off'; input.spellcheck = false;
    input.type = key.includes('Key') ? 'password' : key === 'endpoint' ? 'url' : 'text';
    const element = row('sync', title, detail, input); form.append(element);
  }
  const hint = document.createElement('p'); hint.className = 'settings-hint'; hint.textContent = '配置自动保存。打开应用、每 3 分钟及退出前同步笔记库，包含目录内的图片和附件；冲突无法合并时保留双方副本。移动端进入后台时尝试同步。'; form.append(hint);
  const status = document.createElement('p'); status.className = 'settings-sync-status'; status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
  const actions = document.createElement('div'); actions.className = 'settings-sync-actions'; actions.innerHTML = '<button type="submit" class="primary-button">立即同步</button>'; form.append(actions, status);
  function values() { return Object.fromEntries(new FormData(form)) as S3Settings; }
  let saving = Promise.resolve(), revision = 0, disposed = false, profileLoaded = false, syncing = false, submitting = false;
  function syncError(error: unknown) {
    let text = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请重试';
    for (const secret of [values().accessKeyId, values().secretAccessKey]) if (secret) text = text.replaceAll(secret, '[已隐藏]');
    status.textContent = text; status.dataset.error = 'true'; status.scrollIntoView?.({ block: 'nearest' });
  }
  form.addEventListener('input', () => {
    const current = ++revision, next = values();
    status.textContent = '正在保存…'; status.dataset.error = 'false';
    saving = sync.save(next);
    void saving.then(() => { if (!disposed && revision === current) status.textContent = ''; }).catch(error => { if (!disposed && revision === current) syncError(error); });
  });
  async function run() {
    if (submitting || syncing) return;
    submitting = true; actions.querySelector('button')!.disabled = true;
    try {
      await saving;
      const settings = values();
      const missing = [...form.querySelectorAll<HTMLInputElement>('input')].find(input => !input.value.trim());
      if (missing) { missing.focus(); throw new Error(`请填写${missing.labels?.[0]?.querySelector('span')?.textContent ?? '完整连接配置'}`); }
      if (!form.checkValidity()) throw new Error('服务地址格式无效，请输入完整的 HTTPS 地址');
      await sync.run(settings);
    }
    catch (e) { syncError(e); }
    finally { submitting = false; actions.querySelector('button')!.disabled = !profileLoaded || syncing; }
  }
  form.onsubmit = event => { event.preventDefault(); void run(); };
  for (const control of form.querySelectorAll<HTMLInputElement | HTMLButtonElement>('input,button')) control.disabled = true;
  const unsubscribeSync = sync.subscribe(value => {
    syncing = value.busy;
    status.textContent = value.message; status.dataset.error = String(value.error); if (value.busy) form.setAttribute('aria-busy', 'true'); else form.removeAttribute('aria-busy');
    for (const control of form.querySelectorAll<HTMLInputElement | HTMLButtonElement>('input,button')) control.disabled = !profileLoaded || value.busy;
  });
  const loaded = sync.load().then(config => {
    profileLoaded = true;
    for (const [key, value] of Object.entries(config)) { const input = form.elements.namedItem(key); if (input instanceof HTMLInputElement) input.value = value; }
    for (const control of form.querySelectorAll<HTMLInputElement | HTMLButtonElement>('input,button')) control.disabled = syncing;
  }).catch(syncError);
  root.querySelector<HTMLButtonElement>('.settings-reset')!.onclick = async () => {
    try {
      preferences.update({}, true);
      const shortcut = recorder ? await recorder.reset() : true;
      message.textContent = shortcut ? '已恢复默认设置；笔记位置和同步连接保持原样' : '已恢复外观和编辑器设置；唤起快捷键未更新，请查看提示'; message.dataset.error = String(!shortcut);
    } catch { error('无法保存设置，请检查存储空间'); }
  };
  function updateControls() {
    for (const [key, control] of controls) {
      if (control instanceof HTMLInputElement) control.checked = Boolean(preferences.value[key]);
      else { const value = String(preferences.value[key]); if (![...control.options].some(option => option.value === value)) control.add(new Option(value, value)); control.value = value; }
    }
    for (const choice of choices) choice.update();
  }
  const unsubscribe = preferences.subscribe(updateControls);
  const nativeError = (event: Event) => error((event as CustomEvent<string>).detail);
  window.addEventListener('settings-error', nativeError);
  updateControls(); select('general'); if (preferences.error) error(preferences.error); if (!mobile) void loadFonts();
  return { select, loaded, destroy() { disposed = true; for (const choice of choices) choice.destroy(); recorder?.destroy(); if (locationListener) window.removeEventListener('focus', locationListener); unsubscribe(); unsubscribeSync(); window.removeEventListener('settings-error', nativeError); root.replaceChildren(); } };
}
