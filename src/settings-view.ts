import { invoke } from '@tauri-apps/api/core';
import { homeDir } from '@tauri-apps/api/path';
import { formatStorageBytes, libraryDailyChoices, libraryLocation, libraryStorageUsage, type StorageUsage } from './library-settings';
import { dailyFolderName, dailyTitle, dailyTitleStyles, type DailyNoteChoices } from './daily-note';
import { createShortcutRecorder } from './shortcut-recorder';
import { settingsSelect } from './settings-select';
import { glassToggle } from './glass-controls';
import { mobile, native, noteTextSizes, preferences, syncIntervals, type Preferences } from './preferences';
import type { SyncClient } from './sync-session';
import { syncMode, type S3Settings, type SyncMode } from './sync-store';
import './settings.css';
import { themeNames, themePalette, type Theme } from './themes';
import { isServerLibrary } from './server-library';
import { mountClientAccount, type CloudSyncAccess } from './client-account';
import { version } from '../package.json';

export type SettingsSection = 'account' | 'general' | 'appearance' | 'shortcuts' | 'ai' | 'today' | 'storage' | 'help' | 'personal' | 'signIn' | 'sync';
const sections: [SettingsSection, string, string?][] = [
  ['account', '账户', 'M20 21v-2a7 7 0 0 0-14 0v2M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8'],
  ['general', '通用', 'M3 6h7m4 0h7M3 12h11m4 0h3M3 18h3m4 0h11M10 4v4m4 2v4M6 16v4'],
  ['appearance', '外观', 'M3 20 9 4l6 16M5 15h8m3 5 4-11 4 11m-6-4h5'],
  ['shortcuts', '键盘快捷键', 'M9 7H6a3 3 0 1 1 3-3v16a3 3 0 1 1-3-3h12a3 3 0 1 1-3 3V4a3 3 0 1 1 3 3H9'],
  ['ai', 'MCP 配置', 'm10 13 4-4m-5 7-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0m0 10a4 4 0 0 0 6 0l5-5a4 4 0 0 0-6-6l-2 2'],
  ['today', '今日笔记', 'M4 5h16v16H4zM8 3v4m8-4v4M4 10h16m-12 4h1m3 0h1m3 0h1m-9 4h1m3 0h1'],
  ['storage', '存储与同步', 'm4 5-2 10v5h20v-5L20 5zM2 15h20m-5 2h2'],
  ['help', '帮助与反馈', 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20m-3-14a3 3 0 1 1 4 3c-1 1-1 2-1 3m0 3h.01'],
  ['personal', '个人信息'], ['signIn', '登录与安全'],
];
const settingsParents: Partial<Record<SettingsSection, SettingsSection>> = {
  personal: 'account', signIn: 'account',
};
const nestedSections = Object.keys(settingsParents) as SettingsSection[];
const backButton = (label: string, forward = false) => `<button type="button" class="settings-${forward ? 'forward' : 'back'}" aria-label="${label}"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${forward ? 'm9 5 7 7-7 7' : 'm15 5-7 7 7 7'}"/></svg></button>`;
export function mountSettings(root: HTMLElement, sync: SyncClient, close: () => void, openSection?: (section: SettingsSection) => void,
  loadDaily: () => DailyNoteChoices | Promise<DailyNoteChoices> = native && !mobile ? libraryDailyChoices : () => ({ folders: [] })) {
  const serverLibrary = isServerLibrary();
  const clientAccount = native && !serverLibrary;
  const visibleSections = sections.filter(([key]) => !mobile
    || !['ai', 'shortcuts', ...(clientAccount ? [] : ['personal', 'signIn'])].includes(key));
  const accountName = serverLibrary ? '服务器笔记库' : '本地账户';
  const sectionTitle = (key: SettingsSection | undefined) => key === 'storage' && (serverLibrary || clientAccount)
    ? '存储' : key === 'account' && clientAccount ? '个人资料'
      : mobile && key === 'help' ? '关于轻羽' : visibleSections.find(([section]) => section === key)?.[1] ?? '';
  root.classList.add('settings');
  root.classList.toggle('settings-mobile', mobile);
  root.removeAttribute('data-settings-detail');
  root.innerHTML = mobile ? `<main class="settings-stack">
    <div class="settings-home">
      <header class="settings-header"><button type="button" class="settings-done" aria-label="关闭设置">完成</button></header>
      <div class="settings-pages">
        <h1 class="settings-title" tabindex="-1" autofocus>设置</h1>
        <nav class="settings-categories" aria-label="设置分类"><div class="settings-category-group"></div></nav>
      </div>
    </div>
    <div class="settings-detail" inert>
      <header class="settings-header">${backButton('返回设置')}<h1 class="settings-detail-title"></h1></header>
      <div class="settings-pages"></div>
    </div>
  </main>` : `<aside class="settings-sidebar">
    <div class="settings-sidebar-titlebar" data-tauri-drag-region></div>
    <label class="settings-search"><span aria-hidden="true">⌕</span><input type="search" placeholder="搜索" aria-label="搜索设置"></label>
    <nav class="settings-tabs" role="tablist" aria-label="设置分类" aria-orientation="vertical"></nav>
    <nav class="settings-search-results" aria-label="搜索结果" hidden></nav>
  </aside><header class="settings-header" data-tauri-drag-region>
    <div class="settings-navigation">${backButton('后退')}${backButton('前进', true)}</div>
    <div class="settings-title-path">
      <nav class="settings-breadcrumb" aria-label="设置路径" hidden>
        <button type="button"></button>
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="m4 2.5 3.5 3.5L4 9.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </nav>
      <h1 class="settings-desktop-title"></h1>
    </div>
    ${native ? '' : '<button class="settings-close" type="button" aria-label="关闭设置"><svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="m3 3 6 6m0-6-6 6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></button>'}
  </header><main class="settings-pages"></main>`;
  root.insertAdjacentHTML('beforeend', '<p class="settings-message" role="status" aria-live="polite"></p>');
  const message = root.querySelector<HTMLElement>('.settings-message')!;
  function error(text: string) { message.textContent = text; message.dataset.error = 'true'; }
  for (const button of root.querySelectorAll<HTMLButtonElement>('.settings-detail .settings-back, .settings-done, .settings-close')) button.onclick = close;
  const navigation = root.querySelector('nav')!;
  const search = root.querySelector<HTMLInputElement>('.settings-search input');
  const results = root.querySelector<HTMLElement>('.settings-search-results');
  const categoryGroup = navigation.querySelector('.settings-category-group');
  const home = root.querySelector<HTMLElement>('.settings-home');
  const detail = root.querySelector<HTMLElement>('.settings-detail');
  const detailTitle = detail?.querySelector<HTMLElement>('.settings-detail-title');
  const desktopTitle = root.querySelector<HTMLElement>('.settings-desktop-title');
  const breadcrumb = root.querySelector<HTMLElement>('.settings-breadcrumb');
  const parentButton = breadcrumb?.querySelector('button');
  const detailBack = detail?.querySelector<HTMLButtonElement>('.settings-back');
  const scroller = root.querySelector<HTMLElement>(mobile ? '.settings-detail .settings-pages' : 'main')!;
  let syncHeading: HTMLElement | undefined;
  let storageLocation: HTMLElement | undefined;
  let refreshSyncProfile: (() => Promise<void>) | undefined;
  let refreshMcp: (() => Promise<void>) | undefined;
  let accountPanel: ReturnType<typeof mountClientAccount> | undefined;
  const cloudStorage = document.createElement('div');
  let syncAccount: CloudSyncAccess = { reason: '正在读取账户…' };
  let refreshSyncControls: (() => void) | undefined;
  const controls = new Map<keyof Preferences, HTMLInputElement | HTMLSelectElement>();
  const choices: ReturnType<typeof settingsSelect>[] = [];
  const pages = new Map<SettingsSection, HTMLElement>();
  const tabs: HTMLButtonElement[] = [];
  const trail: SettingsSection[] = [];
  let position = -1;
  const back = root.querySelector<HTMLButtonElement>('.settings-navigation .settings-back');
  const forward = root.querySelector<HTMLButtonElement>('.settings-forward');
  function step(direction: -1 | 1) {
    const section = trail[position + direction];
    if (!section) return;
    clearSearch(); position += direction; renderSection(section, true);
  }
  function navigateHistory(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
    if (event.key !== '[' && event.key !== ']') return;
    event.preventDefault();
    step(event.key === '[' ? -1 : 1);
  }
  if (back && forward) {
    back.onclick = () => step(-1);
    forward.onclick = () => step(1);
    root.addEventListener('keydown', navigateHistory);
  }
  let selected: SettingsSection | undefined;
  function select(section?: SettingsSection, focus = false) {
    clearSearch();
    if (section === 'sync') { if (serverLibrary) section = 'storage'; }
    else if (!section || !pages.has(section)) section = mobile ? undefined : 'general';
    if (!mobile && section && section !== trail[position]) {
      trail.splice(position + 1, trail.length, section);
      position++;
    }
    return renderSection(section, focus);
  }
  function renderSection(section: SettingsSection | undefined, focus: boolean, preview = false) {
    const previous = selected;
    if (!preview) selected = section;
    const pageSection = section === 'sync' ? 'storage' : section;
    if (!preview) accountPanel?.show(pageSection === 'storage' && cloudStorage.hidden ? undefined : pageSection);
    if (!preview && pageSection === 'today') void updateDailyChoices();
    if (!preview && pageSection === 'storage') refreshStorage();
    if (!preview && pageSection === 'ai') void refreshMcp?.();
    const scrollToSection = () => {
      scroller.scrollTop = 0;
      if (section === 'sync' && !clientAccount) syncHeading?.scrollIntoView?.({ block: 'start' });
    };
    if (detail && home) {
      if (section === previous) {
        if (section === 'sync') scrollToSection();
        return section;
      }
      if (section) {
        for (const [key, page] of pages) page.hidden = key !== pageSection;
        detailTitle!.textContent = sectionTitle(pageSection);
      }
      home.inert = Boolean(section); detail.inert = !section;
      root.toggleAttribute('data-settings-detail', Boolean(section));
      if (section) scrollToSection();
      if (focus) (section ? detailBack : tabs.find(tab => tab.dataset.section === (previous === 'sync' ? 'storage' : previous)))?.focus({ preventScroll: true });
      return section;
    }
    for (const [key, page] of pages) { page.hidden = key !== pageSection; page.setAttribute('aria-hidden', String(key !== pageSection)); }
    desktopTitle!.textContent = sectionTitle(pageSection);
    const parent = pageSection && settingsParents[pageSection];
    breadcrumb!.hidden = !parent;
    parentButton!.textContent = parent ? sectionTitle(parent) : '';
    parentButton!.onclick = parent ? () => select(parent, true) : null;
    back!.disabled = position <= 0;
    forward!.disabled = position >= trail.length - 1;
    scrollToSection();
    const tabSection = parent ?? pageSection;
    for (const tab of tabs) {
      const active = tab.dataset.section === tabSection;
      tab.setAttribute('aria-selected', String(active)); tab.tabIndex = active ? 0 : -1;
      if (active && focus && !tab.hidden) tab.focus();
    }
    return section;
  }
  for (const [key, , icon] of visibleSections) {
    const title = sectionTitle(key);
    const tab = document.createElement('button'); tab.type = 'button'; tab.id = `settings-tab-${key}`;
    if (mobile) tab.classList.add('settings-disclosure');
    tab.textContent = title; tab.dataset.section = key; tab.setAttribute('aria-controls', `settings-panel-${key}`);
    tab.onclick = () => navigateTo(key);
    if (mobile && key === 'account') {
      tab.ariaLabel = accountName;
      tab.innerHTML = `<span class="settings-avatar" aria-hidden="true">Q</span><span><strong>${accountName}</strong><small>${sectionTitle('storage')}</small></span>`;
    } else if (icon) tab.insertAdjacentHTML('afterbegin', `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${icon}"/></svg>`);
    tabs.push(tab);
    if (categoryGroup && key === 'account') categoryGroup.before(tab);
    else if (!nestedSections.includes(key)) ((key === 'general' || key === 'appearance') && categoryGroup ? categoryGroup : navigation).append(tab);
    if (key === 'account') tab.classList.add('settings-account-link');
    if (['ai', 'storage', 'help'].includes(key)) tab.classList.add('settings-group-start');
    const page = document.createElement('section'); page.id = `settings-panel-${key}`; page.className = 'settings-page';
    if (mobile) page.hidden = true;
    else {
      tab.role = 'tab'; page.role = 'tabpanel'; page.tabIndex = 0;
      if (nestedSections.includes(key)) page.ariaLabel = title;
      else page.setAttribute('aria-labelledby', tab.id);
    }
    pages.set(key, page); scroller.append(page);
  }
  function navigateTo(section: SettingsSection) { if (mobile && openSection) openSection(section); else select(section, mobile); }
  if (!mobile) navigation.addEventListener('keydown', event => {
    const available = tabs.filter(tab => tab.isConnected && !tab.hidden);
    const e = event as KeyboardEvent, index = available.indexOf(event.target as HTMLButtonElement), count = available.length;
    const next = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? (index + 1) % count
      : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? (index + count - 1) % count
      : e.key === 'Home' ? 0 : e.key === 'End' ? count - 1 : -1;
    if (next >= 0 && count) { e.preventDefault(); select(available[next].dataset.section as SettingsSection, true); }
  });
  function clearSearch() {
    if (!search || !results || results.hidden) return;
    search.value = '';
    results.hidden = true;
    results.replaceChildren();
    navigation.hidden = false;
  }
  if (search && results) {
    let previousScroll = 0;
    const restorePage = () => { renderSection(selected, false, true); scroller.scrollTop = previousScroll; };
    search.addEventListener('input', () => {
      const query = search.value.trim().toLowerCase();
      if (!query) {
        if (!results.hidden) { clearSearch(); restorePage(); }
        return;
      }
      if (results.hidden) previousScroll = scroller.scrollTop;
      navigation.hidden = true;
      results.hidden = false;
      results.replaceChildren();
      const matches = [];
      for (const [section, page] of pages) {
        for (const row of page.querySelectorAll<HTMLElement>('.settings-row, h2')) {
          const hidden = row.closest('[hidden]');
          if (hidden && hidden !== page && page.contains(hidden)) continue;
          const title = (row.querySelector('.settings-label > span') ?? row).textContent?.trim() ?? '';
          const target = row.dataset.settingsTarget as SettingsSection | undefined;
          const destination = target ?? section;
          const category = sectionTitle(destination === 'sync' ? 'storage' : destination);
          if (!title || !`${category} ${row.textContent}`.toLowerCase().includes(query)) continue;
          const normalized = title.toLowerCase();
          matches.push({ title, category, destination, row: target ? undefined : row,
            rank: normalized === query ? 0 : normalized.includes(query) ? 1 : 2 });
        }
      }
      matches.sort((a, b) => a.rank - b.rank);
      for (const [index, match] of matches.entries()) {
        const result = document.createElement('button');
        result.type = 'button';
        const icon = tabs.find(tab => tab.dataset.section === (match.destination === 'sync' ? 'storage' : match.destination))?.querySelector('svg');
        if (icon) result.append(icon.cloneNode(true));
        const copy = document.createElement('span');
        const title = document.createElement('span'), category = document.createElement('small');
        title.textContent = match.title; category.textContent = match.category;
        copy.append(title, category); result.append(copy);
        if (index === 0) result.setAttribute('aria-current', 'true');
        result.onclick = () => {
          select(match.destination);
          pages.get(match.destination === 'sync' ? 'storage' : match.destination)?.focus({ preventScroll: true });
          match.row?.scrollIntoView?.({ block: 'nearest' });
        };
        results.append(result);
      }
      if (matches.length) renderSection(matches[0].destination, false, true);
      else {
        const empty = document.createElement('p');
        empty.setAttribute('role', 'status'); empty.textContent = '没有匹配的设置'; results.append(empty);
        restorePage();
      }
    });
    search.addEventListener('keydown', event => {
      if (results.hidden || event.isComposing) return;
      if (event.key === 'Enter') { event.preventDefault(); results.querySelector('button')?.click(); }
      else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); clearSearch(); restorePage(); }
    });
  }
  let rowId = 0;
  function row(section: SettingsSection, title: string, detail: string, control: HTMLElement, group = false) {
    const row = document.createElement('div'); row.className = `settings-row${group ? ' settings-group' : ''}`;
    const label = document.createElement('label'); label.className = 'settings-label';
    const heading = document.createElement('span'); heading.textContent = title;
    const description = document.createElement('small'); description.textContent = detail; description.id = `settings-detail-${section}-${rowId++}`;
    label.append(heading, description);
    const input = control.matches('input, select') ? control : control.querySelector('input, select');
    if (input) { input.id ||= `${description.id}-control`; label.htmlFor = input.id; input.setAttribute('aria-describedby', description.id); }
    row.append(label, control instanceof HTMLInputElement && control.type === 'checkbox' ? glassToggle(control) : control);
    pages.get(section)!.append(row); return row;
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
    if (control instanceof HTMLSelectElement) decorateSelect(element, control, title);
    return control;
  }
  function decorateSelect(element: HTMLElement, control: HTMLSelectElement, title: string) {
    const choice = settingsSelect(control, title, mobile); choices.push(choice); element.append(choice.element);
    element.querySelector('label')!.htmlFor = choice.id;
    return choice;
  }
  let recorder: ReturnType<typeof createShortcutRecorder> | undefined;
  let locationListener: (() => void) | undefined;
  function heading(section: SettingsSection, text: string) {
    const heading = document.createElement('h2'); heading.textContent = text; pages.get(section)!.append(heading); return heading;
  }
  function hint(section: SettingsSection, text = '此功能尚未接入，相关操作暂不可用。') {
    const hint = document.createElement('p'); hint.className = 'settings-hint'; hint.textContent = text; pages.get(section)!.append(hint);
  }
  function referenceRow(section: SettingsSection, title: string, detail = '', value = '—', target?: SettingsSection) {
    const control = document.createElement('button'); control.type = 'button'; control.className = target ? 'settings-control settings-disclosure' : 'settings-control';
    if (!target) control.textContent = value;
    control.disabled = !target; control.ariaLabel = title;
    if (target) control.onclick = () => navigateTo(target);
    const element = row(section, title, detail, control); element.classList.add('settings-reference-row');
    if (target) { element.classList.add('settings-link-row'); element.dataset.settingsTarget = target; element.onclick = () => navigateTo(target); control.onclick = event => { event.stopPropagation(); navigateTo(target); }; }
    return element;
  }
  function referenceSwitch(section: SettingsSection, title: string, detail = '') {
    const control = document.createElement('input'); control.type = 'checkbox'; control.role = 'switch'; control.className = 'settings-control'; control.disabled = true;
    row(section, title, detail, control).classList.add('settings-reference-row');
  }
  function versionRow(section: SettingsSection, title: string) {
    const value = document.createElement('span'); value.className = 'settings-usage-value'; value.textContent = version;
    row(section, title, '', value).classList.add('settings-reference-row');
  }
  if (clientAccount) accountPanel = mountClientAccount({
    account: pages.get('account')!, personal: pages.get('personal')!, signIn: pages.get('signIn')!, storage: cloudStorage,
  }, root.querySelector<HTMLButtonElement>('#settings-tab-account')!, navigateTo, access => {
    syncAccount = access;
    refreshSyncControls?.();
  });
  else if (!mobile) {
    const identity = document.createElement('div'); identity.className = 'settings-identity';
    identity.innerHTML = '<span class="settings-avatar" aria-hidden="true">Q</span><strong>本地账户</strong><span>账户服务尚未接入</span>';
    pages.get('account')!.append(identity);
    referenceRow('account', '个人信息', '', '', 'personal');
    referenceRow('account', '登录与安全', '', '', 'signIn');
    heading('account', '设备'); referenceRow('account', '本机', '设备管理尚未接入');
    referenceRow('account', '退出登录…', '', '退出登录…');
    referenceRow('personal', '姓名', '', '—'); referenceRow('personal', '电子邮箱', '', '—'); hint('personal');
    referenceRow('signIn', '密码', '', '更改…'); referenceRow('signIn', '删除账户', '', '删除…'); hint('signIn');
  }
  if (!clientAccount) referenceRow('account', sectionTitle('storage'), serverLibrary ? '服务器笔记库' : mobile ? '本机笔记库与 S3 同步' : 'S3 同步设置', '', serverLibrary ? 'storage' : 'sync');
  if (mobile) versionRow('general', '轻羽');
  else {
    referenceRow('general', '轻羽', '软件更新', '检查更新…');
    referenceRow('general', '更新内容', '', '↗');
    heading('general', '此 Mac'); referenceSwitch('general', '在菜单栏显示', '关闭窗口后仍可新建或查找笔记。'); referenceSwitch('general', '登录时打开'); hint('general');
  }
  heading('general', '任务清单'); setting('general', 'sortCheckedItems', '已完成的项目移到底部', '');
  heading('general', '编辑器');
  if (!mobile) setting('general', 'buttonDisplay', '按钮显示', '控制编辑区顶部工具按钮的显示方式', [['always', '始终显示'], ['hover', '悬停时显示']]);
  if (!mobile && native) setting('general', 'alwaysOnTop', '窗口置顶', '让轻羽窗口保持在其他窗口上方', undefined, true);
  setting('general', 'previewWidth', '正文宽度', '笔记编辑区的正文宽度', [600, 800, 1000, 1200, 1400, 0].map(width => [width, width ? `${width} px` : '铺满窗口']));
  const mcpPage = pages.get('ai');
  if (mcpPage && clientAccount) {
    const toggle = document.createElement('button');
    toggle.type = 'button'; toggle.className = 'settings-control';
    toggle.disabled = true; toggle.textContent = '启动服务';
    const state = row('ai', 'API 服务', '正在读取状态…', toggle).querySelector('small')!;
    hint('ai', '启动后，本机 AI 工具可读取和编辑当前笔记库。复制的配置包含令牌，请只发给信任的工具。');
    mcpPage.insertAdjacentHTML('beforeend', `
      <button type="button" class="settings-control" id="settings-mcp-copy" disabled>复制 MCP 配置</button>
      <textarea id="settings-mcp-config" rows="4" aria-label="MCP 配置" readonly hidden></textarea>
      <p class="settings-hint" role="status"></p>`);
    const copy = mcpPage.querySelector<HTMLButtonElement>('#settings-mcp-copy')!;
    const config = mcpPage.querySelector<HTMLTextAreaElement>('#settings-mcp-config')!;
    const message = mcpPage.querySelector('[role=status]')!;
    let connection: { url: string; token: string } | null = null;
    let busy = false;
    async function updateMcp(enabled?: boolean) {
      if (busy) return;
      busy = toggle.disabled = copy.disabled = true;
      config.hidden = true; config.value = '';
      message.textContent = '';
      try {
        connection = await invoke<typeof connection>('local_mcp', { enabled });
        state.textContent = connection ? `运行中 · ${connection.url}` : '已停止';
      } catch (error) {
        message.textContent = String(error);
        if (enabled === undefined) state.textContent = '无法读取状态';
      } finally {
        busy = toggle.disabled = false;
        toggle.textContent = connection ? '停止服务' : '启动服务';
        copy.disabled = !connection;
      }
    }
    refreshMcp = updateMcp;
    toggle.onclick = () => { void updateMcp(!connection); };
    copy.onclick = async () => {
      if (!connection) return;
      const { url, token } = connection;
      const text = `[mcp_servers.qingyu]\nurl = ${JSON.stringify(url)}\nhttp_headers = { Authorization = ${JSON.stringify(`Bearer ${token}`)} }`;
      try {
        await navigator.clipboard.writeText(text);
        message.textContent = '已复制，可以粘贴给 AI 工具。';
      } catch {
        if (connection?.token !== token || copy.disabled) return;
        config.value = text; config.hidden = false; config.focus(); config.select();
        message.textContent = '已选中配置，请使用系统复制快捷键。';
      }
    };
  } else if (mcpPage) {
    if (serverLibrary) {
      heading('ai', 'MCP 地址');
      hint('ai', new URL('/mcp', location.href).href);
      hint('ai', '由服务器部署方管理 API 服务和令牌。MCP 客户端使用 Streamable HTTP，并通过 Authorization 请求头发送 Bearer 令牌。');
    } else {
      hint('ai', '请在轻羽桌面应用中启动本地 MCP 服务。');
    }
  }
  const todayFolder = setting('today', 'todayFolder', '文件夹', '', [['', dailyFolderName]]) as HTMLSelectElement;
  const todayTitle = setting('today', 'todayTitle', '标题', '', dailyTitleStyles.map(style => [style, dailyTitle(new Date(), style)])) as HTMLSelectElement;
  hint('today', '每天用一篇笔记记录当天的内容。');
  setting('today', 'todayOpenOnLaunch', '启动时打开今日笔记', '', undefined, true);
  async function updateDailyChoices() {
    try {
      const { folders } = await loadDaily();
      if (disposed || selected !== 'today') return;
      const current = preferences.value.todayFolder;
      todayFolder.replaceChildren(new Option(dailyFolderName, ''), ...folders.map(([id, title]) => new Option(title, id)));
      if (current && !folders.some(([id]) => id === current)) todayFolder.add(new Option('当前选择已不可用', current, false, true));
      const now = new Date();
      for (const option of todayTitle.options) option.text = dailyTitle(now, option.value as Preferences['todayTitle']);
      updateControls();
    } catch (reason) {
      if (!disposed && selected === 'today') error(reason instanceof Error ? reason.message : '无法读取今日笔记设置');
    }
  }
  pages.get('help')!.insertAdjacentHTML('beforeend', '<div class="settings-help-intro"><span class="settings-avatar" aria-hidden="true">Q</span><div><strong>轻羽</strong><p>用轻羽记录和整理笔记。</p></div></div>');
  if (mobile) versionRow('help', '版本');
  else {
    heading('shortcuts', '笔记');
    for (const [title, key] of [['新建笔记', '⌘N'], ['新建文件夹', '⇧⌘N'], ['搜索笔记', '⇧⌘F'], ['重命名', '⌘R'], ['制作副本', '⌘D'], ['移到废纸篓', '⌘⌫']]) {
      const value = document.createElement('kbd'); value.textContent = key; row('shortcuts', title, '', value).classList.add('settings-reference-row');
    }
    heading('shortcuts', '格式');
    for (const [title, key] of [['粗体', '⌘B'], ['斜体', '⌘I'], ['二级标题', '⇧⌘H'], ['任务清单', '⇧⌘L'], ['插入日期', '⇧⌘D'], ['跳转到笔记／插入链接', '⌘K'], ['上移项目', '⌃⌘↑'], ['下移项目', '⌃⌘↓']]) {
      const value = document.createElement('kbd'); value.textContent = key; row('shortcuts', title, '', value).classList.add('settings-reference-row');
    }
    referenceRow('help', '联系我们', '', '↗'); referenceRow('help', '常见问题', '', '↗');
    referenceRow('help', '键盘快捷键', '', '', 'shortcuts');
    hint('help', '帮助页面尚未接入。');
  }
  if (serverLibrary) row('storage', '笔记位置', '服务器挂载的笔记目录；当前浏览器只保留界面设置和未提交草稿', document.createElement('span'));
  if (native && !mobile) {
    const button = document.createElement('button'); button.type = 'button'; button.className = 'settings-control'; button.textContent = '更改'; button.setAttribute('aria-label', '更改笔记位置'); button.disabled = true;
    const element = row('storage', '笔记位置', '正在读取…', button);
    if (clientAccount) {
      element.classList.add('settings-reference-row');
      storageLocation = element;
    }
    const detail = element.querySelector('small')!;
    let current = '', home = '', changing = false, reading = false;
    function display(path: string) {
      current = path; const base = home.replace(/[\\/]+$/, '');
      const shown = base && (path === base || path.startsWith(base + '/') || path.startsWith(base + '\\')) ? '~' + path.slice(base.length) : path || '尚未打开笔记文件夹';
      const split = Math.max(0, shown.lastIndexOf('/'), shown.lastIndexOf('\\'));
      const prefix = document.createElement('span'), tail = document.createElement('span');
      prefix.textContent = shown.slice(0, split); tail.textContent = shown.slice(split);
      detail.className = 'settings-path'; detail.replaceChildren(prefix, tail); detail.title = path;
    }
    const refreshLocation = async () => {
      if (changing || reading) return;
      reading = true; button.disabled = true;
      try { [current, home] = await Promise.all([libraryLocation(), homeDir().catch(() => '')]); display(current); }
      catch (e) {
        detail.classList.remove('settings-path');
        detail.textContent = '暂时无法读取笔记位置';
        detail.title = e instanceof Error ? e.message : detail.textContent;
      }
      finally { reading = false; button.disabled = false; }
    };
    button.onclick = async () => {
      if (changing) return;
      changing = true; button.disabled = true;
      try {
        const previous = current, path = await libraryLocation(true);
        display(path);
        message.dataset.error = 'false'; message.textContent = path !== previous ? '已切换笔记位置' : '';
        if (path !== previous) void updateStorageUsage();
      }
      catch (e) { error(e instanceof Error ? e.message : '无法更改笔记位置'); }
      finally { changing = false; button.disabled = false; }
    };
    locationListener = () => { void refreshLocation(); };
    window.addEventListener('focus', locationListener); void refreshLocation();
  }
  const appearancePage = pages.get('appearance')!;
  appearancePage.classList.add('appearance-page');
  const modeHeading = document.createElement('h2'); modeHeading.textContent = '模式';
  const modeOptions = document.createElement('div'); modeOptions.className = 'appearance-modes'; modeOptions.role = 'group'; modeOptions.ariaLabel = '模式';
  const modeWindow = (tone: 'light' | 'dark') => `<span class="mode-window" data-tone="${tone}"><span class="mode-mini-sidebar"><i></i><span><b></b></span><span><b></b></span><span><b></b></span></span><span class="mode-mini-note"><b></b><i></i><i></i><i></i><i></i></span></span>`;
  for (const [value, title, symbol] of [['system', '跟随系统', '◐'], ['light', '浅色', '☀'], ['dark', '深色', '☾']] as const) {
    const button = document.createElement('button'); button.type = 'button'; button.dataset.appearance = value;
    button.innerHTML = `<span class="mode-picture" data-mode="${value}" aria-hidden="true">${value === 'system' ? modeWindow('light') + modeWindow('dark') : modeWindow(value)}</span><span><span class="mode-label-icon" aria-hidden="true">${symbol}</span>${title}</span>`;
    button.onclick = () => { try { preferences.update({ appearance: value }); } catch (e) { error(e instanceof Error ? e.message : '无法保存外观'); } };
    modeOptions.append(button);
  }
  appearancePage.append(modeHeading, modeOptions);
  const themeHeading = document.createElement('h2'); themeHeading.textContent = '主题';
  const themeOptions = document.createElement('div'); themeOptions.className = 'appearance-themes'; themeOptions.role = 'group'; themeOptions.ariaLabel = '主题';
  for (const [value, title] of Object.entries(themeNames)) {
    const button = document.createElement('button'); button.type = 'button'; button.dataset.colorTheme = value;
    const pair = document.createElement('span'); pair.className = 'theme-pair'; pair.ariaHidden = 'true';
    for (const dark of [false, true]) {
      const colors = themePalette(value as Theme, dark), swatch = document.createElement('span'); swatch.className = 'theme-swatch';
      swatch.style.setProperty('--swatch-page', colors.paper); swatch.style.setProperty('--swatch-sidebar', colors.sidebar); swatch.style.setProperty('--swatch-accent', colors.brand);
      swatch.dataset.dark = String(dark); swatch.innerHTML = `<i>${dark ? '☾' : '☀'}</i>`; pair.append(swatch);
    }
    const label = document.createElement('span'); label.textContent = title; button.append(pair, label);
    button.onclick = () => { try { preferences.update({ theme: value as Theme }); } catch (e) { error(e instanceof Error ? e.message : '无法保存主题'); } };
    themeOptions.append(button);
  }
  appearancePage.append(themeHeading, themeOptions);
  const modeFootnote = document.createElement('p'); modeFootnote.className = 'settings-hint'; modeFootnote.textContent = '外观按设备单独设置。'; appearancePage.append(modeFootnote);
  const textHeading = document.createElement('h2'); textHeading.textContent = '文字'; appearancePage.append(textHeading);
  const textGroup = document.createElement('div'); textGroup.className = 'appearance-text-group';
  const face = setting('appearance', 'previewFont', '字体', '', [['system','默认'],['ui-serif','衬线'],['ui-rounded','圆体'],['ui-monospace','等宽']]);
  const faceRow = face.closest('.settings-row')!;
  textGroup.append(faceRow);
  const sizeSlider = document.createElement('input'); sizeSlider.type = 'range'; sizeSlider.name = 'previewFontSize'; sizeSlider.step = '1';
  const sizeValue = document.createElement('span'); sizeValue.ariaHidden = 'true';
  const track = document.createElement('span'); track.className = 'appearance-size-track';
  track.append(sizeSlider);
  track.insertAdjacentHTML('beforeend', `<span class="appearance-size-stops" aria-hidden="true">${'<i></i>'.repeat(noteTextSizes.length)}</span>`);
  function updateSize() {
    const size = preferences.value.previewFontSize;
    const index = noteTextSizes.findIndex(stop => stop === size);
    sizeSlider.value = String(index);
    track.style.setProperty('--size-progress', `${index * 100 / (noteTextSizes.length - 1)}%`);
    sizeValue.textContent = `${size} pt`; sizeSlider.setAttribute('aria-valuetext', sizeValue.textContent);
  }
  if (!mobile) {
    const size = document.createElement('div'); size.className = 'appearance-size';
    const small = document.createElement('span'), large = document.createElement('span');
    small.textContent = large.textContent = 'A'; small.ariaHidden = large.ariaHidden = 'true';
    size.append(small, track, large, sizeValue);
    sizeSlider.min = '0'; sizeSlider.max = String(noteTextSizes.length - 1);
    sizeSlider.oninput = () => {
      try { preferences.update({ previewFontSize: noteTextSizes[sizeSlider.valueAsNumber] }); message.textContent = ''; }
      catch (e) { updateSize(); error(e instanceof Error ? e.message : '无法保存字号'); }
    };
    textGroup.append(row('appearance', '正文字号', '', size));
  }
  const sample = document.createElement('div'); sample.className = 'appearance-text-sample'; sample.innerHTML = '<strong>里斯本</strong><p>五月的里斯本，彩砖、电车和刚出炉的蛋挞。</p>';
  textGroup.append(sample); appearancePage.append(textGroup);
  if (native && !mobile) {
    const note = document.createElement('p'); note.className = 'settings-shortcut-status'; note.dataset.error = 'true'; note.setAttribute('role', 'alert');
    recorder = createShortcutRecorder(text => { note.textContent = text; });
    row('shortcuts', '唤起快捷键', '在任何应用里呼出轻羽', recorder.control);
    pages.get('shortcuts')!.append(note);
  }
  let disposed = false;
  let unsubscribeSync = () => {};
  let loaded = Promise.resolve();
  const storagePage = pages.get('storage')!;
  let syncPanel: HTMLElement | undefined;
  if (!serverLibrary) {
    storagePage.classList.add('settings-storage-page');
    const mode = document.createElement('select'); mode.name = 'storageMode';
    const modeStatus = document.createElement('p'); modeStatus.className = 'settings-sync-status';
    modeStatus.role = 'status'; modeStatus.dataset.error = 'true';
    let modeChoice: ReturnType<typeof settingsSelect> | undefined;
    if (clientAccount) {
      mode.add(new Option('轻羽云存储', 'cloud'));
      mode.add(new Option('自定义 S3', 's3'));
      const group = document.createElement('div'); group.className = 'client-list';
      const entry = row('storage', '存储方式', '', mode);
      modeChoice = decorateSelect(entry, mode, '存储方式');
      group.append(entry); storagePage.append(group, modeStatus, cloudStorage);
    }
    syncHeading = heading('storage', 'S3 同步'); syncHeading.id = 'settings-sync-heading';
    syncPanel = document.createElement('section'); storagePage.append(syncPanel);
    const form = document.createElement('form'); form.className = 'settings-sync-form'; form.noValidate = true; form.setAttribute('aria-labelledby', syncHeading.id);
    syncPanel.append(syncHeading, form);
    const fields = form.appendChild(document.createElement('div')); fields.className = 'client-list';
    for (const [key, title, detail] of [
      ['endpoint', '服务地址', '支持 S3 协议的 HTTPS 地址'], ['bucket', 'Bucket', '存储桶名称'], ['region', '区域', '默认 us-east-1'],
      ['prefix', '同步前缀', '不同笔记库请使用不同前缀'], ['accessKeyId', 'Access Key ID', 'S3 访问标识'], ['secretAccessKey', 'Secret Access Key', 'S3 访问密钥'],
    ]) {
      const input = document.createElement('input'); input.name = key; input.className = 'settings-control'; input.required = true; input.maxLength = 4096; input.autocomplete = 'off'; input.spellcheck = false;
      input.type = key.includes('Key') ? 'password' : key === 'endpoint' ? 'url' : 'text';
      const element = row('storage', title, detail, input); fields.append(element);
    }
    const syncGroup = document.createElement('div'); syncGroup.className = 'client-list';
    heading('storage', '同步');
    const auto = document.createElement('input'); auto.type = 'checkbox'; auto.name = 'automaticSync'; auto.role = 'switch';
    syncGroup.append(row('storage', '自动同步', '开启后会在打开、退出应用时，以及按下方间隔同步。', auto));
    const interval = setting('storage', 'syncInterval', '同步间隔', '', syncIntervals.map(minutes => [minutes, `${minutes} 分钟`]));
    syncGroup.append(interval.closest('.settings-row')!);
    const hint = document.createElement('p'); hint.className = 'settings-hint'; hint.textContent = '同步笔记、文件夹和附件。发生冲突时保留双方副本。';
    const status = document.createElement('p'); status.className = 'settings-sync-status'; status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
    const submit = document.createElement('button'); submit.type = 'button'; submit.className = 'settings-sync-now'; submit.textContent = '立即同步';
    const summary = row('storage', '同步', '尚未同步', submit);
    const syncNote = summary.querySelector('small')!;
    syncGroup.append(summary); storagePage.append(syncGroup, hint, status);
    const inputs = [...form.querySelectorAll('input')];
    function values() { return Object.fromEntries(inputs.map(input => [input.name, input.value])) as S3Settings; }
    let saving = Promise.resolve(), revision = 0, profileLoaded = false, syncing = false, submitting = false, switching = false;
    let activeMode: SyncMode = clientAccount ? 'cloud' : 's3';
    let lastSyncedAt: number | undefined;
    let lastSyncedAccount: string | undefined;
    const s3 = syncPanel;
    function updateSyncControls() {
      const disabled = !profileLoaded || syncing || switching;
      const cloud = activeMode === 'cloud';
      auto.checked = cloud ? Boolean(syncAccount.accountId && preferences.value.autoSyncCloudAccount === syncAccount.accountId) : preferences.value.autoSyncS3;
      auto.disabled = disabled || (cloud && !syncAccount.accountId);
      for (const input of inputs) input.disabled = disabled || activeMode !== 's3';
      submit.disabled = disabled || submitting || (cloud && !syncAccount.accountId);
      const lastSync = lastSyncedAccount === (cloud ? syncAccount.accountId : undefined) ? lastSyncedAt : undefined;
      syncNote.textContent = cloud && syncAccount.reason ? syncAccount.reason : syncing ? '正在同步…'
        : lastSync ? `上次同步 ${new Date(lastSync).toLocaleString('zh-CN', { dateStyle: 'short', timeStyle: 'short' })}` : '尚未同步';
      mode.disabled = disabled || submitting;
      mode.value = activeMode;
      modeChoice?.update();
      s3.hidden = activeMode !== 's3';
      cloudStorage.hidden = activeMode !== 'cloud';
    }
    refreshSyncControls = updateSyncControls;
    auto.onchange = () => {
      try {
        if (activeMode === 's3') preferences.update({ autoSyncS3: auto.checked });
        else if (syncAccount.accountId) preferences.update({ autoSyncCloudAccount: auto.checked ? syncAccount.accountId : '' });
      } catch (e) { syncError(e, Boolean(modeChoice)); }
      updateSyncControls();
    };
    mode.onchange = async () => {
      const next = mode.value as SyncMode;
      if (mode.disabled || next === activeMode) return;
      const profile = { ...values(), mode: next };
      switching = true; updateSyncControls();
      try {
        await sync.save(profile);
        if (disposed) return;
        activeMode = next;
        lastSyncedAt = undefined;
        modeStatus.textContent = ''; status.textContent = '';
        if (selected === 'storage' || selected === 'sync') accountPanel?.show(next === 'cloud' ? 'storage' : undefined);
      } catch (e) {
        if (!disposed) syncError(e, true);
      } finally { switching = false; if (!disposed) updateSyncControls(); }
    };
    function syncError(error: unknown, modeError = false) {
      let text = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请重试';
      for (const secret of [values().accessKeyId, values().secretAccessKey]) if (secret) text = text.replaceAll(secret, '[已隐藏]');
      if (modeError) {
        modeStatus.textContent = text;
      } else {
        status.textContent = text; status.dataset.error = 'true'; status.scrollIntoView?.({ block: 'nearest' });
      }
    }
    form.addEventListener('input', () => {
      if (!profileLoaded || switching || activeMode !== 's3') return;
      const current = ++revision, next = values();
      status.textContent = '正在保存…'; status.dataset.error = 'false';
      saving = sync.save(next);
      void saving.then(() => { if (!disposed && revision === current) status.textContent = ''; }).catch(error => { if (!disposed && revision === current) syncError(error); });
    });
    async function run() {
      const cloud = activeMode === 'cloud';
      const accountId = cloud ? syncAccount.accountId : undefined;
      if (!profileLoaded || submitting || syncing || switching || (cloud && !accountId)) return;
      submitting = true; updateSyncControls();
      try {
        await saving;
        const settings = values();
        if (cloud) await sync.run(settings, accountId);
        else {
          const missing = inputs.find(input => !input.value.trim());
          if (missing) { missing.focus(); throw new Error(`请填写${missing.labels?.[0]?.querySelector('span')?.textContent ?? '完整连接配置'}`); }
          if (!form.checkValidity()) throw new Error('服务地址格式无效，请输入完整的 HTTPS 地址');
          await sync.run(settings);
        }
      }
      catch (e) { syncError(e); }
      finally { submitting = false; updateSyncControls(); }
    }
    submit.onclick = () => void run();
    form.onsubmit = event => { event.preventDefault(); void run(); };
    updateSyncControls();
    unsubscribeSync = sync.subscribe(value => {
      const finished = syncing && !value.busy;
      syncing = value.busy;
      lastSyncedAt = value.lastSyncedAt;
      lastSyncedAccount = value.lastSyncedAccount;
      status.textContent = value.message; status.dataset.error = String(value.error); if (value.busy) form.setAttribute('aria-busy', 'true'); else form.removeAttribute('aria-busy');
      updateSyncControls();
      if (finished && !value.error && (selected === 'storage' || selected === 'sync')) void updateStorageUsage();
    });
    let pendingProfile: Promise<void> | undefined;
    refreshSyncProfile = () => {
      if (profileLoaded) return loaded;
      return pendingProfile ??= sync.load().then(config => {
        if (disposed) return;
        profileLoaded = true;
        for (const input of inputs) input.value = config[input.name as keyof Omit<S3Settings, 'mode'>] ?? '';
        if (clientAccount) activeMode = syncMode(config);
        modeStatus.textContent = '';
        updateSyncControls();
        if (activeMode === 's3' && (selected === 'storage' || selected === 'sync')) accountPanel?.show(undefined);
        if (!syncing) { status.textContent = ''; status.dataset.error = 'false'; }
      }).catch(e => { if (!disposed) syncError(e, !!modeChoice); }).finally(() => { pendingProfile = undefined; });
    };
    loaded = refreshSyncProfile();
  }
  const usageGroup = document.createElement('div'); usageGroup.className = 'settings-storage-usage';
  const total = document.createElement('output');
  total.className = 'settings-usage-value';
  total.textContent = '正在统计…';
  const usageRow = row('storage', '存储用量', native || serverLibrary ? '当前笔记库' : '此浏览器已保存的笔记正文', total);
  usageRow.classList.add('settings-reference-row');
  const meter = document.createElement('div'); meter.className = 'settings-storage-meter'; meter.setAttribute('aria-hidden', 'true');
  usageGroup.append(usageRow, meter);
  const usageItems = ([
    ['notes', '笔记与应用'], ['files', '附件与文件'], ['trash', '废纸篓'],
  ] as const).map(([key, title]) => {
    const value = document.createElement('span'); value.className = 'settings-usage-value';
    const entry = row('storage', title, '', value);
    entry.classList.add('settings-reference-row', 'settings-usage-row');
    entry.dataset.usage = key;
    const bar = document.createElement('span'); bar.dataset.usage = key; meter.append(bar);
    usageGroup.append(entry);
    return { key, value, bar };
  });
  storagePage.insertBefore(usageGroup, clientAccount ? null : syncPanel ?? null);
  if (clientAccount) {
    storagePage.insertBefore(heading('storage', '本机笔记库'), usageGroup);
    if (storageLocation) usageGroup.append(storageLocation);
  }
  let usageRevision = 0;
  async function updateStorageUsage() {
    const revision = ++usageRevision;
    total.textContent = '正在统计…';
    let result: StorageUsage | undefined;
    try { result = await libraryStorageUsage(); } catch { /* The sync section remains usable. */ }
    if (disposed || revision !== usageRevision) return;
    const bytes = result ? result.notes + result.files + result.trash : 0;
    total.textContent = result ? formatStorageBytes(bytes) : '无法读取用量';
    for (const { key, value, bar } of usageItems) {
      value.textContent = formatStorageBytes(result?.[key]);
      bar.style.width = bytes && result ? (100 * result[key] / bytes) + '%' : '0%';
    }
  }
  function refreshStorage() {
    void updateStorageUsage();
    void refreshSyncProfile?.();
  }
  const refreshOnFocus = () => {
    if (selected === 'storage' || selected === 'sync') refreshStorage();
    if (selected === 'ai') void refreshMcp?.();
  };
  window.addEventListener('focus', refreshOnFocus);
  function updateControls() {
    if (!mobile) updateSize();
    for (const dark of [false, true]) {
      const colors = themePalette(preferences.value.theme, dark);
      const mode = dark ? 'dark' : 'light';
      for (const [role, color] of Object.entries({ page: colors.paper, sidebar: colors.sidebar, ink: colors.ink, tint: colors.brand, muted: colors.muted, line: colors.line, selected: colors['folder-selection'] })) {
        modeOptions.style.setProperty(`--thumb-${mode}-${role}`, color);
      }
    }
    for (const button of themeOptions.querySelectorAll<HTMLButtonElement>('button')) button.setAttribute('aria-pressed', String(button.dataset.colorTheme === preferences.value.theme));
    for (const button of modeOptions.querySelectorAll<HTMLButtonElement>('button')) button.setAttribute('aria-pressed', String(button.dataset.appearance === preferences.value.appearance));
    for (const [key, control] of controls) {
      if (control instanceof HTMLInputElement) control.checked = Boolean(preferences.value[key]);
      else { const value = String(preferences.value[key]); if (![...control.options].some(option => option.value === value)) control.add(new Option(value, value)); control.value = value; }
    }
    for (const choice of choices) choice.update();
    refreshSyncControls?.();
  }
  const unsubscribe = preferences.subscribe(updateControls);
  const nativeError = (event: Event) => error((event as CustomEvent<string>).detail);
  window.addEventListener('settings-error', nativeError);
  updateControls(); if (preferences.error) error(preferences.error);
  if (!mobile) select('general');
  return { select, loaded, destroy() { disposed = true; accountPanel?.destroy(); for (const choice of choices) choice.destroy(); recorder?.destroy(); if (locationListener) window.removeEventListener('focus', locationListener); unsubscribe(); unsubscribeSync(); window.removeEventListener('settings-error', nativeError); window.removeEventListener('focus', refreshOnFocus); root.removeEventListener('keydown', navigateHistory); root.replaceChildren(); } };
}
