import { invoke } from '@tauri-apps/api/core';
import { homeDir } from '@tauri-apps/api/path';
import { formatStorageBytes, libraryDailyChoices, libraryLocation, libraryStorageUsage, type StorageUsage } from './library-settings';
import { dailyFolderName, dailyTitle, dailyTitleStyles, type DailyNoteChoices } from './daily-note';
import { createShortcutRecorder } from './shortcut-recorder';
import { settingsSelect } from './settings-select';
import { glassToggle } from './glass-controls';
import { mobile, native, noteTextSizes, preferences, syncIntervals, type Preferences } from './preferences';
import type { SyncClient } from './sync-session';
import { defaultAssetDownload, hasS3Connection, s3Defaults, syncMode, type S3Settings, type SyncMode } from './sync-store';
import './settings.css';
import { themeNames, themePalette, type Theme } from './themes';
import { isServerLibrary } from './server-library';
import { guestAccountIcon, mountClientAccount, type CloudSyncAccess } from './client-account';
import { settingsList, settingsListButton } from './settings-list';
import { accountAvailable } from './account-client';
import { createServerSyncClient } from './server-sync-client';
import { alertActions, showAlert } from './alert-dialog';
import { version } from '../package.json';
import { supportedLanguages } from './vendor/markra/shared/i18n';
import { formatLabel, markupLabel, bindLabels, localizeStatus, localizeSyncError, translatedLabel, uiText, workflowText, type UiMessage, type WorkflowMessage } from './ui-language';

const githubRepo = 'appdev/QingYu';
const stableReleaseTag = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

export type SettingsSection = 'account' | 'general' | 'appearance' | 'shortcuts' | 'ai' | 'today' | 'storage' | 'help' | 'personal' | 'signIn' | 'sync';
const sections: [SettingsSection, UiMessage, string?][] = [
  ['account', 'account'],
  ['general', 'settings.categories.general', 'M3 6h7m4 0h7M3 12h11m4 0h3M3 18h3m4 0h11M10 4v4m4 2v4M6 16v4'],
  ['appearance', 'settings.categories.appearance', 'M3 20 9 4l6 16M5 15h8m3 5 4-11 4 11m-6-4h5'],
  ['shortcuts', 'shortcuts', 'M9 7H6a3 3 0 1 1 3-3v16a3 3 0 1 1-3-3h12a3 3 0 1 1-3 3V4a3 3 0 1 1 3 3H9'],
  ['ai', 'mcp', 'm10 13 4-4m-5 7-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0m0 10a4 4 0 0 0 6 0l5-5a4 4 0 0 0-6-6l-2 2'],
  ['today', 'todayNote', 'M4 5h16v16H4zM8 3v4m8-4v4M4 10h16m-12 4h1m3 0h1m3 0h1m-9 4h1m3 0h1'],
  ['storage', 'storageSync', 'm4 5-2 10v5h20v-5L20 5zM2 15h20m-5 2h2'],
  ['help', 'help', 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20m-3-14a3 3 0 1 1 4 3c-1 1-1 2-1 3m0 3h.01'],
  ['personal', 'personal'], ['signIn', 'signIn'],
];
const settingsParents: Partial<Record<SettingsSection, SettingsSection>> = {
  personal: 'account', signIn: 'account',
};
const nestedSections = Object.keys(settingsParents) as SettingsSection[];
type SettingLabel = string | readonly [UiMessage | WorkflowMessage];
const labelText = (label: SettingLabel) => typeof label === 'string' ? label : translatedLabel(label[0]);
function setLabel(element: HTMLElement, label: SettingLabel) {
  element.textContent = labelText(label);
  if (typeof label !== 'string') element.dataset.i18n = label[0];
}
const backButton = (label: UiMessage, forward = false) => `<button type="button" class="settings-${forward ? 'forward' : 'back'}" data-i18n-aria-label="${label}"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${forward ? 'm9 5 7 7-7 7' : 'm15 5-7 7 7 7'}"/></svg></button>`;
export function mountSettings(root: HTMLElement, syncSource: SyncClient | undefined, close: () => void, openSection?: (section: SettingsSection) => void,
  loadDaily: () => DailyNoteChoices | Promise<DailyNoteChoices> = native && !mobile ? libraryDailyChoices : () => ({ folders: [] })) {
  const serverLibrary = isServerLibrary();
  // 网页客户端的同步配置保存在部署端的配置目录，由它运行同步；后面的同步界面和 App 是同一段代码
  const sync = serverLibrary ? createServerSyncClient() : syncSource;
  const clientAccount = native && !serverLibrary;
  // 轻羽账号 App 和网页客户端都有，同一个账户页；账号请求经 account-client 交给 Rust
  const signInAccount = accountAvailable();
  const visibleSections = sections.filter(([key]) => {
    if (key === 'account') return signInAccount || (mobile && sync && !serverLibrary);
    if (key === 'personal' || key === 'signIn') return signInAccount;
    if (key === 'ai') return !mobile && (native || serverLibrary);
    return key !== 'shortcuts' || !mobile;
  });
  const sectionMessage = (key: SettingsSection | undefined): UiMessage | undefined => key === 'account' && signInAccount ? 'profile'
      : mobile && key === 'help' ? 'about'
      : key === 'storage' && !sync && !serverLibrary ? 'settings.categories.storage' : visibleSections.find(([section]) => section === key)?.[1];
  const sectionTitle = (key: SettingsSection | undefined) => {
    const message = sectionMessage(key);
    return message ? uiText(message) : '';
  };
  root.classList.add('settings');
  root.classList.toggle('settings-mobile', mobile);
  root.removeAttribute('data-settings-detail');
  root.innerHTML = mobile ? `<main class="settings-stack">
    <div class="settings-home">
      <header class="settings-header"><button type="button" class="button floating-button settings-done" data-i18n-aria-label="closeSettings" data-i18n="done"></button></header>
      <div class="settings-pages">
        <h1 class="settings-title" tabindex="-1" autofocus data-i18n="settings.title"></h1>
        <nav class="settings-categories" data-i18n-aria-label="settingsCategories"><div class="settings-category-group"></div></nav>
      </div>
    </div>
    <div class="settings-detail" inert>
      <header class="settings-header">${backButton('backToSettings')}<h1 class="settings-detail-title"></h1></header>
      <div class="settings-pages"></div>
    </div>
  </main>` : `<aside class="settings-sidebar">
    <div class="settings-sidebar-titlebar" data-tauri-drag-region></div>
    <label class="settings-search"><span aria-hidden="true">⌕</span><input type="search" data-i18n-placeholder="app.searchPlaceholder" data-i18n-aria-label="searchSettings"></label>
    <nav class="settings-tabs" role="tablist" data-i18n-aria-label="settingsCategories" aria-orientation="vertical"></nav>
    <nav class="settings-search-results" aria-label="${markupLabel('app.workspaceSearch.results')}" hidden></nav>
  </aside><header class="settings-header" data-tauri-drag-region>
    <div class="settings-navigation">${backButton('back')}${backButton('forward', true)}</div>
    <div class="settings-title-path">
      <nav class="settings-breadcrumb" aria-label="${markupLabel('设置路径')}" hidden>
        <button type="button"></button>
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="m4 2.5 3.5 3.5L4 9.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </nav>
      <h1 class="settings-desktop-title"></h1>
    </div>
    ${native ? '' : '<button class="settings-close" type="button" data-i18n-aria-label="closeSettings"><svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="m3 3 6 6m0-6-6 6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></button>'}
  </header><main class="settings-pages"></main>`;
  root.insertAdjacentHTML('beforeend', '<p class="settings-message" role="status" aria-live="polite"></p>');
  const message = root.querySelector<HTMLElement>('.settings-message')!;
  function error(text: string) { message.textContent = localizeStatus(text); message.dataset.error = 'true'; }
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
  let refreshMcpLanguage: (() => void) | undefined;
  let accountPanel: ReturnType<typeof mountClientAccount> | undefined;
  const cloudStorage = document.createElement('div');
  let syncAccount: CloudSyncAccess = { reason: '正在读取账户…' };
  let refreshSyncControls: (() => void) | undefined;
  let refreshSyncLanguage: (() => void) | undefined;
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
    if (section === 'sync') { if (!sync || serverLibrary) section = 'storage'; }
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
    if (key === 'account') {
      const avatar = `<span class="settings-avatar" aria-hidden="true">${guestAccountIcon}</span>`;
      if (mobile) {
        tab.dataset.i18nAriaLabel = 'localAccount';
        tab.innerHTML = `${avatar}<span><strong data-i18n="localAccount"></strong><small data-i18n="${sectionMessage('storage')}"></small></span>`;
      } else {
        const text = document.createElement('span');
        setLabel(text, [sectionMessage(key)!]);
        tab.replaceChildren(text);
        tab.insertAdjacentHTML('afterbegin', avatar);
      }
    } else {
      const text = document.createElement('span');
      text.dataset.i18n = sectionMessage(key);
      tab.replaceChildren(text);
      if (icon) tab.insertAdjacentHTML('afterbegin', `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${icon}"/></svg>`);
    }
    tabs.push(tab);
    if (categoryGroup && key === 'account') categoryGroup.before(tab);
    else if (!nestedSections.includes(key)) ((key === 'general' || key === 'appearance') && categoryGroup ? categoryGroup : navigation).append(tab);
    if (key === 'account') tab.classList.add('settings-account-link');
    if (['ai', 'storage', 'help'].includes(key)) tab.classList.add('settings-group-start');
    const page = document.createElement('section'); page.id = `settings-panel-${key}`; page.className = 'settings-page';
    if (mobile) page.hidden = true;
    else {
      tab.role = 'tab'; page.role = 'tabpanel'; page.tabIndex = 0;
      if (nestedSections.includes(key)) page.dataset.i18nAriaLabel = sectionMessage(key);
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
        empty.setAttribute('role', 'status'); empty.textContent = uiText('noSettings'); results.append(empty);
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
  function row(section: SettingsSection, title: SettingLabel, detail: SettingLabel, control: HTMLElement, group = false) {
    const row = document.createElement('div'); row.className = `settings-row${group ? ' settings-group' : ''}`;
    const label = document.createElement('label'); label.className = 'settings-label';
    const heading = document.createElement('span'); setLabel(heading, title);
    const description = document.createElement('small'); setLabel(description, detail); description.id = `settings-detail-${section}-${rowId++}`;
    label.append(heading, description);
    const input = control.matches('input, select') ? control : control.querySelector('input, select');
    if (input) { input.id ||= `${description.id}-control`; label.htmlFor = input.id; input.setAttribute('aria-describedby', description.id); }
    row.append(label, control instanceof HTMLInputElement && control.type === 'checkbox' ? glassToggle(control) : control);
    pages.get(section)!.append(row); return row;
  }
  function setting(section: SettingsSection, key: keyof Preferences, title: SettingLabel, detail: SettingLabel, options?: [string | number, SettingLabel][], group = false) {
    const control = options ? document.createElement('select') : document.createElement('input');
    control.className = 'settings-control'; control.name = key;
    if (control instanceof HTMLSelectElement) for (const [value, text] of options!) {
      const option = new Option('', String(value));
      setLabel(option, text);
      control.add(option);
    }
    else { control.type = 'checkbox'; control.setAttribute('role', 'switch'); }
    controls.set(key, control);
    control.onchange = () => {
      const value = control instanceof HTMLInputElement ? control.checked : typeof preferences.value[key] === 'number' ? Number(control.value) : control.value;
      try { preferences.update({ [key]: value }); message.textContent = ''; }
      catch (e) { error(e instanceof Error ? e.message : '无法保存设置'); updateControls(); }
    };
    const element = row(section, title, detail, control, group);
    if (control instanceof HTMLSelectElement) decorateSelect(element, control, () => labelText(title));
    return control;
  }
  function decorateSelect(element: HTMLElement, control: HTMLSelectElement, title: string | (() => string)) {
    const choice = settingsSelect(control, title, mobile); choices.push(choice); element.append(choice.element);
    element.querySelector('label')!.htmlFor = choice.id;
    return choice;
  }
  let recorder: ReturnType<typeof createShortcutRecorder> | undefined;
  let locationListener: (() => void) | undefined;
  function heading(section: SettingsSection, text: SettingLabel) {
    const heading = document.createElement('h2'); setLabel(heading, text); pages.get(section)!.append(heading); return heading;
  }
  function hint(section: SettingsSection, text: SettingLabel = ['此功能尚未接入，相关操作暂不可用。']) {
    const hint = document.createElement('p'); hint.className = 'settings-hint'; setLabel(hint, text); pages.get(section)!.append(hint);
  }
  function referenceRow(section: SettingsSection, title: SettingLabel, detail: SettingLabel = '', value: SettingLabel = '—', target?: SettingsSection) {
    const control = document.createElement('button'); control.type = 'button'; control.className = target ? 'settings-control settings-disclosure' : 'settings-control';
    if (!target) setLabel(control, value);
    control.disabled = !target; control.ariaLabel = labelText(title);
    if (typeof title !== 'string') control.dataset.i18nAriaLabel = title[0];
    if (target) control.onclick = () => navigateTo(target);
    const element = row(section, title, detail, control); element.classList.add('settings-reference-row');
    if (target) { element.classList.add('settings-link-row'); element.dataset.settingsTarget = target; element.onclick = () => navigateTo(target); control.onclick = event => { event.stopPropagation(); navigateTo(target); }; }
    return element;
  }
  function referenceSwitch(section: SettingsSection, title: SettingLabel, detail = '') {
    const control = document.createElement('input'); control.type = 'checkbox'; control.role = 'switch'; control.className = 'settings-control'; control.disabled = true;
    return row(section, title, detail, control);
  }
  function versionRow(section: SettingsSection, title: SettingLabel) {
    const value = document.createElement('span'); value.className = 'settings-usage-value'; value.textContent = version;
    const element = row(section, title, '', value);
    element.classList.add('settings-reference-row');
    return element;
  }
  if (signInAccount) accountPanel = mountClientAccount({
    account: pages.get('account')!, personal: pages.get('personal')!, signIn: pages.get('signIn')!, storage: cloudStorage,
  }, root.querySelector<HTMLButtonElement>('#settings-tab-account')!, navigateTo, access => {
    syncAccount = access;
    refreshSyncControls?.();
  });
  else if (pages.has('account')) referenceRow('account', [sectionMessage('storage')!], ['本机笔记库与 S3 同步'], '', 'sync');
  const generalPage = pages.get('general')!;
  const language = setting('general', 'language', ['settings.language.title'], ['settings.language.description'],
    [['system', ['systemLanguage']], ...supportedLanguages.map(({ code, label }): [string, string] => [code, label])]);
  generalPage.append(settingsList(language.closest('.settings-row')!));
  if (mobile) generalPage.append(settingsList(versionRow('general', ['轻羽'])));
  else {
    referenceRow('general', ['轻羽'], ['settings.update.title'], ['settings.update.check']);
    referenceRow('general', ['releaseNotes'], '', '↗');
  }
  // 菜单栏和登录时打开依赖桌面系统，网页客户端没有
  if (!mobile && native) {
    const mac = /Mac/.test(navigator.platform);
    heading('general', [mac ? '此 Mac' : '此电脑']);
    const menuBar = setting('general', 'menuBar', [mac ? '在菜单栏显示' : '在系统托盘显示'], ['关闭窗口后仍可新建或查找笔记。']);
    generalPage.append(settingsList(menuBar.closest('.settings-row')!, referenceSwitch('general', ['登录时打开'])));
    hint('general');
  }
  heading('general', '').dataset.i18n = 'checklist';
  const sortCheckedItems = setting('general', 'sortCheckedItems', ['sortCompleted'], '');
  generalPage.append(settingsList(sortCheckedItems.closest('.settings-row')!));
  heading('general', '').dataset.i18n = 'settings.categories.editor';
  const editorGroup = settingsList();
  if (!mobile) {
    const buttonDisplay = setting('general', 'buttonDisplay', ['toolbarVisibility'], ['toolbarVisibilityHint'], [['always', ['alwaysShow']], ['hover', ['onHover']]]);
    editorGroup.append(buttonDisplay.closest('.settings-row')!);
  }
  if (!mobile && native) {
    const alwaysOnTop = setting('general', 'alwaysOnTop', ['keepOnTop'], ['keepOnTopHint']);
    editorGroup.append(alwaysOnTop.closest('.settings-row')!);
  }
  const previewWidth = setting('general', 'previewWidth', ['noteWidth'], ['noteWidthHint'], [600, 800, 1000, 1200, 1400, 0].map(width => [width, width ? `${width} px` : ['fillWindow']]));
  editorGroup.append(previewWidth.closest('.settings-row')!);
  generalPage.append(editorGroup);
  const mcpPage = pages.get('ai');
  // MCP 页各端共用：客户端在本机启停服务、令牌由本机生成；服务器笔记库的服务常驻在服务器上，令牌由部署方提供
  type McpConnection = { url: string; token: string };
  const mcpHost = clientAccount ? {
    toggle: true, initial: undefined as McpConnection | undefined,
    hint: '本机 AI 工具可读取和编辑当前笔记库。服务地址变化后，请重新复制配置。' as const,
    aiHint: '复制的内容包含访问令牌，请只交给信任的工具。' as const,
    read: (enabled?: boolean) => invoke<McpConnection | null>('local_mcp', { enabled }),
  } : serverLibrary ? (() => {
    const server = { url: new URL('/mcp', location.href).href, token: '' };
    return {
      toggle: false, initial: server as McpConnection | undefined,
      hint: 'MCP 服务由服务器部署方管理。客户端通过 Authorization 请求头发送 Bearer 令牌，复制后请把令牌占位换成部署方提供的令牌。' as const,
      aiHint: '令牌需要向部署方获取。' as const,
      read: async (_enabled?: boolean) => server as McpConnection | null,
    };
  })() : undefined;
  if (mcpPage && mcpHost) {
    const host = mcpHost;
    const clients = [
      { name: 'Codex', path: '~/.codex/config.toml', format: 'TOML' },
      { name: 'Claude Code', path: '~/.claude.json', format: 'JSON' },
      { name: 'Cursor', path: '~/.cursor/mcp.json', format: 'JSON' },
    ];
    const toggle = document.createElement('button');
    toggle.type = 'button'; toggle.className = 'button secondary-button';
    toggle.disabled = true; toggle.textContent = translatedLabel('启动服务');
    const state = row('ai', ['MCP 服务'], ['正在读取状态…'], host.toggle ? toggle : document.createElement('span')).querySelector('small')!;
    state.classList.add('settings-mcp-address');
    hint('ai', [host.hint]);
    mcpPage.insertAdjacentHTML('beforeend', `
      <section class="settings-mcp-panel" aria-labelledby="settings-mcp-heading">
        <h2 id="settings-mcp-heading" data-i18n="客户端配置"></h2>
        <div class="settings-mcp-toolbar">
          <label for="settings-mcp-client" data-i18n="客户端"></label>
          <select id="settings-mcp-client">${clients.map(({ name }) => `<option>${name}</option>`).join('')}</select>
          <span class="settings-mcp-transport">Streamable HTTP</span>
          <button type="button" class="button quiet-button" id="settings-mcp-reveal" aria-controls="settings-mcp-preview" disabled></button>
          <button type="button" class="button secondary-button" id="settings-mcp-copy" data-i18n="复制配置" disabled></button>
        </div>
        <textarea class="settings-mcp-preview" id="settings-mcp-preview" rows="3" wrap="off" data-i18n-aria-label="配置预览" data-i18n-placeholder="启动 MCP 服务后显示配置" readonly></textarea>
        <p class="settings-hint" id="settings-mcp-target"></p>
      </section>
      <div class="settings-mcp-ai">
        <div>
          <h2 data-i18n="让 AI 配置 MCP"></h2>
          <p class="settings-hint" id="settings-mcp-ai-hint"></p>
        </div>
        <button type="button" class="button secondary-button" id="settings-mcp-copy-ai" data-i18n="复制给 AI" disabled></button>
      </div>
      <textarea class="settings-mcp-preview" id="settings-mcp-fallback" data-i18n-aria-label="手动复制内容" readonly hidden></textarea>
      <p class="settings-hint settings-mcp-status" role="status"></p>`);
    const client = mcpPage.querySelector<HTMLSelectElement>('#settings-mcp-client')!;
    const choice = settingsSelect(client, () => translatedLabel('客户端'), false);
    mcpPage.querySelector('.settings-mcp-toolbar label')!.after(choice.element);
    const reveal = mcpPage.querySelector<HTMLButtonElement>('#settings-mcp-reveal')!;
    const copy = mcpPage.querySelector<HTMLButtonElement>('#settings-mcp-copy')!;
    const copyAi = mcpPage.querySelector<HTMLButtonElement>('#settings-mcp-copy-ai')!;
    const preview = mcpPage.querySelector<HTMLTextAreaElement>('#settings-mcp-preview')!;
    const target = mcpPage.querySelector('#settings-mcp-target')!;
    const fallback = mcpPage.querySelector<HTMLTextAreaElement>('#settings-mcp-fallback')!;
    const message = mcpPage.querySelector('[role=status]')!;
    let connection: McpConnection | null = host.initial ?? null;
    const aiHint = mcpPage.querySelector('#settings-mcp-ai-hint')!;
    let statusKey: WorkflowMessage = '正在读取状态…';
    let checked = Boolean(connection);
    let messageSource = '';
    delete state.dataset.i18n;
    // 服务器的令牌不在浏览器里，不提供显示令牌
    if (!host.toggle) reveal.remove();
    let busy = false;
    let showToken = false;
    let copyRevision = 0;
    function formatConfig(url: string, token: string) {
      const authorization = `Bearer ${token || `<${workflowText('服务器令牌')}>`}`;
      if (client.value === 'Codex') {
        return `[mcp_servers.qingyu]\nurl = ${JSON.stringify(url)}\nhttp_headers = { Authorization = ${JSON.stringify(authorization)} }`;
      }
      const headers = { Authorization: authorization };
      const qingyu = client.value === 'Claude Code' ? { type: 'http', url, headers } : { url, headers };
      return JSON.stringify({ mcpServers: { qingyu } }, null, 2);
    }
    function clearCopy() {
      copyRevision++;
      fallback.hidden = true; fallback.value = '';
      setMcpMessage('');
    }
    function setMcpMessage(value: string) {
      messageSource = value; message.textContent = localizeStatus(value);
    }
    function renderConfig() {
      aiHint.textContent = formatLabel('复制带配置的提示词，让 AI 合并并验证连接。{hint}', { hint: translatedLabel(host.aiHint) });
      state.textContent = connection ? formatLabel('运行中 · {url}', { url: connection.url })
        : checked ? uiText('app.aiAgentProcessStatusCancelled') : workflowText(statusKey);
      toggle.textContent = workflowText(connection ? '停止服务' : '启动服务');
      setMcpMessage(messageSource);
      const { path, format } = clients[client.selectedIndex];
      reveal.disabled = copy.disabled = copyAi.disabled = busy || !connection;
      reveal.textContent = showToken ? translatedLabel('隐藏令牌') : translatedLabel('显示令牌');
      preview.value = connection && !busy ? formatConfig(connection.url, showToken || !host.toggle ? connection.token : `<${workflowText('令牌已隐藏')}>`) : '';
      preview.rows = Math.max(3, preview.value.split('\n').length);
      target.textContent = host.toggle ? formatLabel('点击「复制配置」获取含完整令牌的 {format} 配置，并合并到 {path}。', { format, path })
        : formatLabel('点击「复制配置」获取 {format} 配置，填入令牌后合并到 {path}。', { format, path });
    }
    async function updateMcp(enabled?: boolean) {
      if (busy) return;
      busy = toggle.disabled = true;
      showToken = false;
      clearCopy(); renderConfig();
      try {
        connection = await host.read(enabled);
        checked = true;
      } catch (error) {
        setMcpMessage(error instanceof Error ? error.message : String(error));
        if (enabled === undefined) { checked = false; statusKey = '无法读取状态'; }
      } finally {
        busy = toggle.disabled = false;
        renderConfig();
      }
    }
    async function copyConfig(forAi: boolean) {
      if (busy || !connection) return;
      clearCopy();
      const revision = copyRevision;
      const { name, path, format } = clients[client.selectedIndex];
      const config = formatConfig(connection.url, connection.token);
      const intro = formatLabel(host.toggle ? '请在运行轻羽的这台电脑上为 {name} 配置轻羽 MCP（Streamable HTTP）。' : '请为 {name} 配置轻羽 MCP（Streamable HTTP）。', { name });
      const instructions = formatLabel('将下面的 qingyu 条目合并到用户配置 {path}，保留其余设置。请勿输出、上传或提交访问令牌。重新加载配置后，仅通过列出 MCP 工具验证连接，请勿读取或修改笔记。', { path });
      const text = forAi ? `${intro}\n${instructions}\n\n\`\`\`${format.toLowerCase()}\n${config}\n\`\`\`` : config;
      try {
        await navigator.clipboard.writeText(text);
        if (revision === copyRevision) setMcpMessage(forAi ? '已复制提示词。' : '已复制配置。');
      } catch {
        if (revision !== copyRevision || selected !== 'ai' || disposed) return;
        fallback.value = text; fallback.hidden = false; fallback.focus(); fallback.select();
        setMcpMessage('已选中复制内容，请使用系统复制快捷键。');
      }
    }
    refreshMcp = updateMcp;
    refreshMcpLanguage = renderConfig;
    toggle.onclick = () => { void updateMcp(!connection); };
    client.onchange = () => { showToken = false; clearCopy(); renderConfig(); };
    reveal.onclick = () => { showToken = !showToken; clearCopy(); renderConfig(); };
    copy.onclick = () => { void copyConfig(false); };
    copyAi.onclick = () => { void copyConfig(true); };
    renderConfig();
  } else if (mcpPage) hint('ai', ['请在轻羽桌面应用中启动本地 MCP 服务。']);
  const todayFolder = setting('today', 'todayFolder', ['folders'], '', [['', [dailyFolderName]]]) as HTMLSelectElement;
  const todayTitle = setting('today', 'todayTitle', ['标题'], '', dailyTitleStyles.map(style => [style, dailyTitle(new Date(), style, document.documentElement.lang || undefined)])) as HTMLSelectElement;
  hint('today', ['每天用一篇笔记记录当天的内容。']);
  setting('today', 'todayOpenOnLaunch', ['启动时打开今日笔记'], '', undefined, true);
  function updateDailyTitleExamples() {
    const now = new Date();
    for (const option of todayTitle.options) option.text = dailyTitle(now, option.value as Preferences['todayTitle'], document.documentElement.lang || undefined);
  }
  async function updateDailyChoices() {
    try {
      const { folders } = await loadDaily();
      if (disposed || selected !== 'today') return;
      const current = preferences.value.todayFolder;
      const defaultFolder = new Option('', ''); setLabel(defaultFolder, [dailyFolderName]);
      todayFolder.replaceChildren(defaultFolder, ...folders.map(([id, title]) => new Option(title, id)));
      if (current && !folders.some(([id]) => id === current)) {
        const missing = new Option('', current, false, true);
        setLabel(missing, ['当前选择已不可用']);
        todayFolder.add(missing);
      }
      updateDailyTitleExamples();
      updateControls();
    } catch (reason) {
      if (!disposed && selected === 'today') error(reason instanceof Error ? reason.message : '无法读取今日笔记设置');
    }
  }
  const help = pages.get('help')!;
  help.insertAdjacentHTML('beforeend', '<div class="settings-help-intro"><img src="/brand/qingyu-app.png" width="52" height="52" alt=""><p><strong>QingYu</strong> — <span data-i18n="轻量的所见即所得 Markdown 编辑器。"></span></p></div>');
  function openGithub(path: string) {
    const url = `https://github.com/${githubRepo}/${path}`;
    if (native) void invoke('plugin:opener|open_url', { url }).catch(() => error('无法打开 GitHub 链接。'));
    else window.open(url, '_blank', 'noopener,noreferrer');
  }
  function helpRow(title: WorkflowMessage, value: string, onSelect: () => void) {
    const button = settingsListButton({ title: '', onSelect });
    setLabel(button.querySelector<HTMLElement>('.settings-label > span')!, [title]);
    const text = document.createElement('span');
    text.textContent = value;
    if (value === '↗') text.setAttribute('aria-hidden', 'true');
    button.append(text);
    return button;
  }
  const checkVersion = helpRow('版本', version, async () => {
    if (checkVersion.disabled) return;
    checkVersion.disabled = true;
    const value = checkVersion.lastElementChild!;
    value.textContent = translatedLabel('正在检查…');
    let upToDate = false;
    let update: string | undefined;
    const request = new AbortController();
    const timeout = window.setTimeout(() => request.abort(), 10_000);
    try {
      const response = await fetch(`https://api.github.com/repos/${githubRepo}/releases/latest`, {
        cache: 'no-cache', signal: request.signal,
      });
      if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
      const { tag_name: tag } = await response.json();
      const latest = typeof tag === 'string' ? stableReleaseTag.exec(tag) : null;
      const current = stableReleaseTag.exec(version);
      if (!latest || !current) throw new Error('Invalid release version');
      const difference = latest.slice(1).map((part, index) => BigInt(part) - BigInt(current[index + 1])).find(part => part !== 0n);
      if (difference !== undefined && difference > 0n) update = latest[0].replace(/^v/, '');
      upToDate = !update;
    } catch {
      // A failed request, including a private repository's 404, gives no version information.
    } finally {
      window.clearTimeout(timeout);
      checkVersion.disabled = false;
      value.textContent = version;
    }
    if (disposed || selected !== 'help') return;
    const dialog = document.createElement('dialog');
    const heading = document.createElement('h2');
    heading.id = 'settings-update-title';
    heading.textContent = update ? formatLabel('发现新版本 {version}', { version: update }) : workflowText(upToDate ? '已是最新版本' : '暂时无法检查更新');
    dialog.setAttribute('aria-labelledby', heading.id);
    const closeButton = document.createElement('button');
    closeButton.textContent = update ? translatedLabel('取消') : translatedLabel('确定');
    closeButton.onclick = () => dialog.close();
    const download = document.createElement('button');
    download.textContent = translatedLabel('前往下载');
    download.onclick = () => { dialog.close(); openGithub('releases/latest'); };
    const primary = update ? download : closeButton;
    dialog.append(heading, update ? alertActions(download, closeButton) : alertActions(closeButton));
    dialog.addEventListener('keydown', event => { if (event.key === 'Escape') event.stopPropagation(); });
    dialog.addEventListener('close', () => { dialog.remove(); checkVersion.focus(); }, { once: true });
    root.append(dialog);
    showAlert(dialog, primary);
  });
  help.append(settingsList(
    checkVersion,
    helpRow('问题反馈', '↗', () => openGithub('issues/new')),
    helpRow('常见问题', '↗', () => openGithub('issues')),
  ));
  if (!mobile) {
    heading('shortcuts', ['笔记']);
    for (const [title, key] of [['新建笔记', '⌘N'], ['app.newMarkdownFolder', '⇧⌘N'], ['searchNotes', '⇧⌘F'], ['重命名', '⌘R'], ['duplicate', '⌘D'], ['moveToTrash', '⌘⌫']] as const) {
      const value = document.createElement('kbd'); value.textContent = key; row('shortcuts', [title], '', value).classList.add('settings-reference-row');
    }
    heading('shortcuts', ['格式']);
    for (const [title, key] of [['粗体', '⌘B'], ['斜体', '⌘I'], ['二级标题', '⇧⌘H'], ['任务清单', '⇧⌘L'], ['插入日期', '⇧⌘D'], ['跳转到笔记／插入链接', '⌘K'], ['上移项目', '⌃⌘↑'], ['下移项目', '⌃⌘↓']] as const) {
      const value = document.createElement('kbd'); value.textContent = key; row('shortcuts', [title], '', value).classList.add('settings-reference-row');
    }
    referenceRow('help', ['shortcuts'], '', '', 'shortcuts');
  }
  if (native && !mobile) {
    const button = document.createElement('button'); button.type = 'button'; button.className = 'settings-control'; setLabel(button, ['更改']); button.dataset.i18nAriaLabel = '更改笔记位置'; button.disabled = true;
    const element = row('storage', ['笔记位置'], '', button);
    if (clientAccount) {
      element.classList.add('settings-reference-row');
      storageLocation = element;
    }
    const detail = element.querySelector('small')!;
    detail.textContent = workflowText('正在读取…');
    let current = '', home = '', changing = false, reading = false;
    function display(path: string) {
      current = path; const base = home.replace(/[\\/]+$/, '');
      const shown = base && (path === base || path.startsWith(base + '/') || path.startsWith(base + '\\')) ? '~' + path.slice(base.length) : path || workflowText('尚未打开笔记文件夹');
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
        detail.textContent = workflowText('暂时无法读取笔记位置');
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
        message.textContent = '';
        if (path !== previous) void updateStorageUsage();
      }
      catch (e) { error(e instanceof Error ? localizeStatus(e.message) : workflowText('无法更改笔记位置')); }
      finally { changing = false; button.disabled = false; }
    };
    locationListener = () => { void refreshLocation(); };
    window.addEventListener('focus', locationListener); void refreshLocation();
  }
  const appearancePage = pages.get('appearance')!;
  appearancePage.classList.add('appearance-page');
  const modeHeading = document.createElement('h2'); modeHeading.dataset.i18n = 'settings.theme.appearanceModeTitle';
  const modeOptions = document.createElement('div'); modeOptions.className = 'appearance-modes'; modeOptions.role = 'group'; modeOptions.dataset.i18nAriaLabel = 'settings.theme.appearanceModeTitle';
  const modeWindow = (tone: 'light' | 'dark') => `<span class="mode-window" data-tone="${tone}"><span class="mode-mini-sidebar"><i></i><span><b></b></span><span><b></b></span><span><b></b></span></span><span class="mode-mini-note"><b></b><i></i><i></i><i></i><i></i></span></span>`;
  for (const [value, symbol] of [['system', '◐'], ['light', '☀'], ['dark', '☾']] as const) {
    const button = document.createElement('button'); button.type = 'button'; button.dataset.appearance = value;
    button.innerHTML = `<span class="mode-picture" data-mode="${value}" aria-hidden="true">${value === 'system' ? modeWindow('light') + modeWindow('dark') : modeWindow(value)}</span><span><span class="mode-label-icon" aria-hidden="true">${symbol}</span><span data-i18n="settings.theme.${value}"></span></span>`;
    button.onclick = () => { try { preferences.update({ appearance: value }); } catch (e) { error(e instanceof Error ? e.message : '无法保存外观'); } };
    modeOptions.append(button);
  }
  appearancePage.append(modeHeading, modeOptions);
  const themeHeading = document.createElement('h2'); themeHeading.dataset.i18n = 'settings.sections.theme';
  const themeOptions = document.createElement('div'); themeOptions.className = 'appearance-themes'; themeOptions.role = 'group'; themeOptions.dataset.i18nAriaLabel = 'settings.sections.theme';
  for (const [value, title] of Object.entries(themeNames)) {
    const button = document.createElement('button'); button.type = 'button'; button.dataset.colorTheme = value;
    const pair = document.createElement('span'); pair.className = 'theme-pair'; pair.ariaHidden = 'true';
    for (const dark of [false, true]) {
      const colors = themePalette(value as Theme, dark), swatch = document.createElement('span'); swatch.className = 'theme-swatch';
      swatch.style.setProperty('--swatch-page', colors.paper); swatch.style.setProperty('--swatch-sidebar', colors.sidebar); swatch.style.setProperty('--swatch-accent', colors.brand);
      swatch.dataset.dark = String(dark); swatch.innerHTML = `<i>${dark ? '☾' : '☀'}</i>`; pair.append(swatch);
    }
    const label = document.createElement('span'); setLabel(label, [title]); button.append(pair, label);
    button.onclick = () => { try { preferences.update({ theme: value as Theme }); } catch (e) { error(e instanceof Error ? e.message : '无法保存主题'); } };
    themeOptions.append(button);
  }
  appearancePage.append(themeHeading, themeOptions);
  const modeFootnote = document.createElement('p'); modeFootnote.className = 'settings-hint'; setLabel(modeFootnote, ['外观按设备单独设置。']); appearancePage.append(modeFootnote);
  const textHeading = document.createElement('h2'); setLabel(textHeading, ['文字']); appearancePage.append(textHeading);
  const textGroup = document.createElement('div'); textGroup.className = 'appearance-text-group';
  const face = setting('appearance', 'previewFont', ['字体'], '', [['system', ['默认']], ['ui-serif', ['衬线']], ['ui-rounded', ['圆体']], ['ui-monospace', ['等宽']]]);
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
    textGroup.append(row('appearance', ['settings.editor.bodyFontSize'], '', size));
  }
  const sample = document.createElement('div'); sample.className = 'appearance-text-sample'; sample.innerHTML = `<strong data-i18n="里斯本"></strong><p data-i18n="五月的里斯本，彩砖、电车和刚出炉的蛋挞。"></p>`;
  textGroup.append(sample); appearancePage.append(textGroup);
  if (native && !mobile) {
    const note = document.createElement('p'); note.className = 'settings-shortcut-status'; note.dataset.error = 'true'; note.setAttribute('role', 'alert');
    recorder = createShortcutRecorder(text => { note.textContent = text; });
    row('shortcuts', ['唤起快捷键'], ['在任何应用里呼出轻羽'], recorder.control);
    pages.get('shortcuts')!.append(note);
  }
  let disposed = false;
  let switchConfirmation: HTMLDialogElement | undefined;
  let passwordDialog: HTMLDialogElement | undefined;
  let unsubscribeSync = () => {};
  let loaded = Promise.resolve();
  const storagePage = pages.get('storage')!;
  let syncPanel: HTMLElement | undefined;
  if (sync) {
    const client = sync;
    storagePage.classList.add('settings-storage-page');
    const mode = document.createElement('select'); mode.name = 'storageMode';
    const modeStatus = document.createElement('p'); modeStatus.className = 'settings-sync-status';
    modeStatus.role = 'status'; modeStatus.dataset.error = 'true';
    let modeChoice: ReturnType<typeof settingsSelect> | undefined;
    if (signInAccount) {
      const cloudOption = new Option('', 'cloud'), s3Option = new Option('', 's3');
      setLabel(cloudOption, ['轻羽云存储']);
      setLabel(s3Option, ['自定义 S3']);
      mode.add(cloudOption);
      mode.add(s3Option);
      const entry = row('storage', ['存储方式'], '', mode);
      modeChoice = decorateSelect(entry, mode, () => workflowText('存储方式'));
      storagePage.append(settingsList(entry), modeStatus, cloudStorage);
    }
    syncHeading = heading('storage', ['S3 同步']); syncHeading.id = 'settings-sync-heading';
    syncPanel = document.createElement('section'); storagePage.append(syncPanel);
    const form = document.createElement('form'); form.className = 'settings-sync-form'; form.noValidate = true; form.setAttribute('aria-labelledby', syncHeading.id);
    syncPanel.append(syncHeading, form);
    const fields = form.appendChild(settingsList());
    for (const [key, title, detail] of [
      ['endpoint', ['服务地址'], ['支持 S3 协议的 HTTPS 地址']], ['bucket', ['存储桶'], ['存储桶名称']], ['region', ['区域'], ['默认 us-east-1']],
      ['prefix', ['同步前缀'], ['不同笔记库请使用不同前缀']], ['accessKeyId', ['访问密钥 ID'], ['S3 访问标识']], ['secretAccessKey', ['访问密钥'], ['S3 访问密钥']],
    ] as const) {
      const input = document.createElement('input'); input.name = key; input.className = 'settings-control'; input.required = true; input.maxLength = 4096; input.autocomplete = 'off'; input.spellcheck = false;
      input.type = key.includes('Key') ? 'password' : key === 'endpoint' ? 'url' : 'text';
      const element = row('storage', title, detail, input); fields.append(element);
    }
    for (const [key, title, detail, min, max] of [
      ['timeout', ['超时（秒）'], ['请求超时（1–300 秒）'], 1, 300],
      ['concurrency', ['并发请求数'], ['并发请求数（1–16）'], 1, 16],
    ] as const) {
      const input = document.createElement('input'); input.name = key; input.type = 'number'; input.className = 'settings-control'; input.required = true;
      input.min = String(min); input.max = String(max); input.step = '1';
      fields.append(row('storage', title, detail, input));
    }
    for (const [key, title, detail, options] of [
      ['forcePathStyle', ['寻址方式'], ['Bucket 的寻址方式'], [['true', ['路径样式']], ['false', ['虚拟主机样式']]]],
      ['tlsVerify', ['TLS 证书校验'], ['默认校验证书。仅对信任的自建服务跳过校验。'], [['true', ['校验证书']], ['false', ['跳过校验']]]],
      ['assetDownload', ['附件下载'], ['按需下载时，小于 1 MB 的附件仍会自动下载'], [['all', ['全部下载']], ['on-demand', ['按需下载']]]],
    ] as const) {
      const select = document.createElement('select'); select.name = key;
      for (const [value, label] of options) {
        const option = new Option('', value); setLabel(option, label); select.add(option);
      }
      const element = row('storage', title, detail, select); fields.append(element);
      decorateSelect(element, select, () => labelText(title));
    }
    const test = document.createElement('button'); test.type = 'button'; test.className = 'button settings-sync-action settings-sync-test'; setLabel(test, ['测试连接']);
    form.append(row('storage', ['连接测试'], ['检查读取、写入、条件写入和删除权限。'], test));
    const syncGroup = settingsList();
    heading('storage', ['同步']);
    const auto = document.createElement('input'); auto.type = 'checkbox'; auto.name = 'automaticSync'; auto.role = 'switch';
    syncGroup.append(row('storage', ['自动同步'], ['开启后会在打开、退出应用时，以及按下方间隔同步。'], auto));
    const interval = setting('storage', 'syncInterval', ['同步间隔'], '', syncIntervals.map(minutes => [minutes, ''])) as HTMLSelectElement;
    syncGroup.append(interval.closest('.settings-row')!);
    const hint = document.createElement('p'); hint.className = 'settings-hint'; setLabel(hint, ['同步笔记、文件夹和附件。发生冲突时两边的版本都会保留。']);
    const status = document.createElement('p'); status.className = 'settings-sync-status'; status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
    const errorBlock = document.createElement('div'); errorBlock.className = 'settings-sync-error'; errorBlock.hidden = true;
    const errorText = document.createElement('pre'); errorText.tabIndex = 0; errorText.setAttribute('data-i18n-aria-label', '连接错误详情');
    const copyError = document.createElement('button'); copyError.type = 'button'; copyError.className = 'button settings-sync-action settings-sync-copy'; copyError.textContent = workflowText('复制错误信息');
    errorBlock.append(errorText, copyError);
    copyError.onclick = async () => {
      try { await navigator.clipboard.writeText(errorText.textContent ?? ''); copyError.textContent = workflowText('已复制'); }
      catch { copyError.textContent = workflowText('复制失败，请选择内容复制'); }
    };
    const submit = document.createElement('button'); submit.type = 'button'; submit.className = 'button settings-sync-action settings-sync-now'; setLabel(submit, ['立即同步']);
    const summary = row('storage', ['同步'], '', submit);
    const syncNote = summary.querySelector('small')!;
    const passwordButton = document.createElement('button'); passwordButton.type = 'button'; passwordButton.className = 'button settings-sync-action';
    setLabel(passwordButton, ['管理密钥']);
    const passwordRow = client.password
      ? row('storage', ['数据仓库密钥'], ['数据仓库用于加密保存数据快照。同步时需要解锁密钥。'], passwordButton)
      : undefined;
    if (passwordRow) syncGroup.append(passwordRow);
    syncGroup.append(summary); storagePage.append(syncGroup, hint, status, errorBlock);
    const inputs = [...form.querySelectorAll<HTMLInputElement | HTMLSelectElement>('input, select')];
    function values() {
      return Object.fromEntries(inputs.map(input => [input.name,
        ['forcePathStyle', 'tlsVerify'].includes(input.name) ? input.value === 'true' : ['timeout', 'concurrency'].includes(input.name) ? Number(input.value) : input.value])) as S3Settings;
    }
    let currentStatus = '', modeErrorText = '', statusFailed = false;
    let updatePermissionLabels: (() => void) | undefined;
    const diagnostic = (value: string) => localizeStatus(value).replaceAll('[已隐藏]', workflowText('[已隐藏]'));
    function showStatus(message: string, failed = false) {
      currentStatus = message;
      statusFailed = failed;
      const label = failed ? localizeSyncError(message) : diagnostic(message);
      status.textContent = failed ? label.split('\n', 1)[0] : label; status.dataset.error = String(failed);
      errorBlock.hidden = !failed || !message;
      errorText.textContent = failed ? label : '';
      copyError.textContent = workflowText('复制错误信息');
    }
    if (native && /Android/i.test(navigator.userAgent)) {
      const permission = (request: boolean) => invoke<boolean | null>('sync_local_network_access', { request });
      void permission(false).then(granted => {
        if (disposed || granted === null) return;
        const button = document.createElement('button'); button.type = 'button'; button.className = 'button secondary-button';
        const access = row('storage', ['局域网访问'], ['连接局域网内的 S3 服务时需要系统授权。'], button);
        const applyAccessLabels = bindLabels(access);
        form.append(access);
        const update = (allowed: boolean) => {
          button.disabled = allowed;
          updatePermissionLabels = () => {
            applyAccessLabels();
            button.textContent = workflowText(allowed ? '已授权' : '授权');
            button.ariaLabel = workflowText(allowed ? '已授权局域网访问' : '授权局域网访问');
          };
          updatePermissionLabels();
        };
        update(granted);
        button.onclick = async () => {
          button.disabled = true;
          try {
            const allowed = await permission(true) === true;
            if (disposed) return;
            update(allowed);
            showStatus(allowed ? '' : '未获局域网权限，可在系统设置中授权轻羽。', !allowed);
          } catch (error) {
            if (disposed) return;
            update(false);
            syncError(error);
          }
        };
      }).catch(() => {});
    }
    let saving = Promise.resolve(), revision = 0, profileLoaded = false, syncing = false, submitting = false, switching = false;
    let activeMode: SyncMode = signInAccount ? 'cloud' : 's3';
    let lastSyncedAt: number | undefined;
    let lastSyncedAccount: string | undefined;
    let unavailable: string | undefined;
    const s3 = syncPanel;
    function updateSyncControls() {
      const disabled = !profileLoaded || syncing || switching;
      const cloud = activeMode === 'cloud';
      const accountId = syncAccount.signedInAccountId ?? syncAccount.accountId;
      const ready = !unavailable && (cloud ? Boolean(syncAccount.accountId) : hasS3Connection(values()));
      auto.checked = cloud ? Boolean(accountId && preferences.value.autoSyncCloudAccount === accountId) : ready && preferences.value.autoSyncS3;
      auto.disabled = disabled || !ready;
      for (const input of inputs) input.disabled = disabled || activeMode !== 's3';
      test.disabled = disabled || submitting || cloud;
      submit.disabled = disabled || submitting || (cloud && !syncAccount.accountId) || Boolean(unavailable);
      passwordButton.disabled = disabled || submitting || !ready;
      if (passwordRow) passwordRow.hidden = !!client.cloudPasswordOnly && !cloud;
      const lastSync = lastSyncedAccount === (cloud ? syncAccount.accountId : undefined) ? lastSyncedAt : undefined;
      syncNote.textContent = unavailable ? diagnostic(unavailable) : cloud && syncAccount.reason ? syncAccount.reason : syncing ? workflowText('正在同步…')
        : lastSync ? formatLabel('上次同步 {date}', { date: new Date(lastSync).toLocaleString(document.documentElement.lang || undefined, { dateStyle: 'short', timeStyle: 'short' }) })
          : workflowText('尚未同步');
      mode.disabled = disabled || submitting;
      mode.value = activeMode;
      modeChoice?.update();
      for (const choice of choices) choice.update();
      s3.hidden = activeMode !== 's3';
      cloudStorage.hidden = activeMode !== 'cloud';
    }
    refreshSyncControls = updateSyncControls;
    refreshSyncLanguage = () => {
      for (const option of interval.options) option.textContent = workflowText('{minutes} 分钟').replace('{minutes}', option.value);
      showStatus(currentStatus, statusFailed);
      if (modeStatus.textContent) modeStatus.textContent = localizeSyncError(modeErrorText);
      updatePermissionLabels?.();
    };
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
        const confirmed = await new Promise<boolean>(resolve => {
          const dialog = document.createElement('dialog'); switchConfirmation = dialog;
          dialog.className = 'client-logout-confirmation settings-provider-confirmation'; dialog.role = 'alertdialog';
          dialog.setAttribute('aria-labelledby', 'settings-provider-title'); dialog.setAttribute('aria-describedby', 'settings-provider-description');
          dialog.innerHTML = '<h2 id="settings-provider-title" data-i18n="切换云存储提供商"></h2><p id="settings-provider-description" data-i18n="切换同步提供商前，需要检查资源和历史快照的完整性，必要时会从当前云端下载缺失内容，可能需要较长时间。是否继续？"></p><div class="client-logout-actions"><button type="button" data-cancel data-i18n="取消"></button><button type="button" data-continue data-i18n="继续"></button></div>';
          bindLabels(dialog)();
          dialog.querySelector<HTMLButtonElement>('[data-cancel]')!.onclick = () => dialog.close('cancel');
          dialog.querySelector<HTMLButtonElement>('[data-continue]')!.onclick = () => dialog.close('continue');
          dialog.onkeydown = event => { if (event.key === 'Escape') event.stopPropagation(); };
          dialog.oncancel = event => { event.preventDefault(); event.stopPropagation(); dialog.close('cancel'); };
          dialog.onclose = () => { resolve(dialog.returnValue === 'continue'); dialog.remove(); switchConfirmation = undefined; };
          root.append(dialog); dialog.showModal(); dialog.querySelector<HTMLButtonElement>('[data-cancel]')!.focus();
        });
        if (!confirmed || disposed) return;
        await saving;
        await client.save(profile);
        if (disposed) return;
        activeMode = next;
        lastSyncedAt = undefined;
        modeStatus.textContent = ''; showStatus('');
        if (selected === 'storage' || selected === 'sync') accountPanel?.show(next === 'cloud' ? 'storage' : undefined);
      } catch (e) {
        if (!disposed) syncError(e, true);
      } finally { switching = false; if (!disposed) updateSyncControls(); }
    };
    function syncError(error: unknown, modeError = false) {
      let text = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请重试';
      for (const secret of [values().accessKeyId, values().secretAccessKey]) if (secret) text = text.replaceAll(secret, '[已隐藏]');
      if (modeError) {
        modeErrorText = text;
        modeStatus.textContent = localizeSyncError(text);
      } else {
        showStatus(text, true); status.scrollIntoView?.({ block: 'nearest' });
      }
    }
    function saveFields() {
      if (!profileLoaded || switching || activeMode !== 's3') return;
      const current = ++revision, next = values();
      showStatus('正在保存…'); updateSyncControls();
      saving = client.save(next);
      void saving.then(() => { if (!disposed && revision === current) showStatus(''); }).catch(error => { if (!disposed && revision === current) syncError(error); });
    }
    passwordButton.onclick = async () => {
      const managePassword = client.password;
      if (!managePassword || passwordButton.disabled) return;
      const settings = values();
      const accountId = activeMode === 'cloud' ? syncAccount.accountId : undefined;
      submitting = true; updateSyncControls();
      try {
        await saving;
        if (activeMode === 's3' && !form.reportValidity()) return;
        const info = await managePassword(settings, accountId);
        if (disposed) return;
        if (info.immutable && info.state === 'unlocked') {
          showStatus('数据仓库密钥已解锁');
          return;
        }
        const dialog = document.createElement('dialog'); passwordDialog = dialog;
        dialog.className = 'alert-dialog';
        dialog.setAttribute('aria-labelledby', 'settings-sync-password-title');
        dialog.innerHTML = `<form class="settings-password-form">
          <h2 id="settings-sync-password-title"></h2>
          <label><span data-i18n="数据仓库密钥"></span><input type="password" required autofocus></label>
          <label data-confirm><span data-i18n="确认数据仓库密钥"></span><input type="password" autocomplete="new-password"></label>
          <div role="alert" hidden></div>
          <button type="button" data-i18n="取消"></button><button type="submit"></button>
        </form>`;
        const locked = info.state === 'locked';
        const title = info.state === 'new' ? '设置数据仓库密钥' : locked ? '输入数据仓库密钥' : '修改或重设数据仓库密钥';
        const passwordForm = dialog.querySelector('form')!;
        const [password, confirmation] = dialog.querySelectorAll('input');
        const [cancel, confirm] = dialog.querySelectorAll('button');
        const message = dialog.querySelector<HTMLElement>('[role="alert"]')!;
        setLabel(dialog.querySelector('h2')!, [title]);
        setLabel(confirm, [locked ? '解锁' : 'menu.saveDocument']);
        passwordForm.append(alertActions(confirm, cancel));
        password!.autocomplete = locked ? 'current-password' : 'new-password';
        confirmation!.required = !locked;
        confirmation!.closest('label')!.hidden = locked;
        bindLabels(dialog)();
        cancel!.onclick = () => dialog.close();
        dialog.onkeydown = event => { if (event.key === 'Escape') event.stopPropagation(); };
        dialog.oncancel = event => { if (confirm!.disabled) event.preventDefault(); };
        dialog.onclose = () => { passwordForm.reset(); dialog.remove(); passwordDialog = undefined; };
        passwordForm.onsubmit = async event => {
          event.preventDefault();
          if (confirm!.disabled) return;
          if (!locked && password!.value.normalize('NFC') !== confirmation!.value.normalize('NFC')) {
            message.hidden = false; message.textContent = workflowText('两次输入的密钥不一致'); confirmation!.focus();
            return;
          }
          confirm!.disabled = cancel!.disabled = password!.disabled = confirmation!.disabled = true;
          message.hidden = true;
          try {
            await managePassword(settings, accountId, { ...info, password: password!.value });
            if (dialog.open) dialog.close();
          } catch (error) {
            message.hidden = false; message.textContent = localizeSyncError(error instanceof Error ? error.message : String(error));
          } finally {
            confirm!.disabled = cancel!.disabled = password!.disabled = confirmation!.disabled = false;
          }
        };
        root.append(dialog); dialog.showModal();
      } catch (error) {
        if (!disposed) syncError(error);
      } finally { submitting = false; if (!disposed) updateSyncControls(); }
    };
    form.addEventListener('input', event => { if (event.target instanceof HTMLInputElement) saveFields(); });
    form.addEventListener('change', event => { if (event.target instanceof HTMLSelectElement) saveFields(); });
    async function run(testConnection = false) {
      const cloud = activeMode === 'cloud';
      const accountId = cloud ? syncAccount.accountId : undefined;
      if (!profileLoaded || submitting || syncing || switching || (cloud && !accountId)) return;
      submitting = true; updateSyncControls();
      try {
        await saving;
        const settings = values();
        if (cloud) await client.run(settings, accountId);
        else {
          const missing = inputs.find(input => !input.value.trim());
          if (missing) {
            missing.focus();
            const field = missing.labels?.[0]?.querySelector('span')?.textContent ?? workflowText('完整连接配置');
            throw new Error(workflowText('请填写{field}').replace('{field}', () => field));
          }
          if (!form.checkValidity()) throw new Error('请检查服务地址、超时和并发请求数的格式与范围');
          if (testConnection) await client.test(settings); else await client.run(settings);
        }
      }
      catch (e) { syncError(e); }
      finally { submitting = false; updateSyncControls(); }
    }
    submit.onclick = () => void run();
    test.onclick = () => void run(true);
    form.onsubmit = event => { event.preventDefault(); void run(); };
    updateSyncControls();
    unsubscribeSync = client.subscribe(value => {
      const finished = syncing && !value.busy;
      syncing = value.busy;
      lastSyncedAt = value.lastSyncedAt;
      lastSyncedAccount = value.lastSyncedAccount;
      unavailable = value.unavailable;
      if (value.mode) activeMode = value.mode;
      showStatus(value.message, value.error); if (value.busy) form.setAttribute('aria-busy', 'true'); else form.removeAttribute('aria-busy');
      updateSyncControls();
      if (finished && !value.error && (selected === 'storage' || selected === 'sync')) void updateStorageUsage();
    });
    let pendingProfile: Promise<void> | undefined;
    refreshSyncProfile = () => {
      if (profileLoaded) return loaded;
      return pendingProfile ??= client.load().then(config => {
        if (disposed) return;
        profileLoaded = true;
        for (const input of inputs) input.value = String(config[input.name as keyof S3Settings]
          ?? (input.name === 'assetDownload' ? defaultAssetDownload(mobile) : s3Defaults[input.name as keyof typeof s3Defaults]) ?? '');
        if (signInAccount) activeMode = syncMode(config);
        modeStatus.textContent = '';
        updateSyncControls();
        if (activeMode === 's3' && (selected === 'storage' || selected === 'sync')) accountPanel?.show(undefined);
      }).catch(e => { if (!disposed) syncError(e, !!modeChoice); }).finally(() => { pendingProfile = undefined; });
    };
    loaded = refreshSyncProfile();
  }
  const usageGroup = document.createElement('div'); usageGroup.className = 'settings-storage-usage';
  const total = document.createElement('output');
  total.className = 'settings-usage-value';
  total.textContent = workflowText('正在统计…');
  const usageRow = row('storage', ['存储用量'], native || serverLibrary ? ['当前笔记库'] : ['此浏览器已保存的笔记正文'], total);
  usageRow.classList.add('settings-reference-row');
  const meter = document.createElement('div'); meter.className = 'settings-storage-meter'; meter.setAttribute('aria-hidden', 'true');
  usageGroup.append(usageRow, meter);
  const usageItems = ([
    ['notes', ['笔记与应用']], ['files', ['附件与文件']], ['trash', ['废纸篓']],
  ] as const).map(([key, title]) => {
    const value = document.createElement('span'); value.className = 'settings-usage-value';
    const entry = row('storage', title, '', value);
    entry.classList.add('settings-reference-row', 'settings-usage-row');
    entry.dataset.usage = key;
    const bar = document.createElement('span'); bar.dataset.usage = key; meter.append(bar);
    usageGroup.append(entry);
    return { key, value, bar };
  });
  // 笔记库这一段各端共用：标题 + 用量卡片，笔记位置放在卡片最后一行
  const libraryTitle = signInAccount ? '本机笔记库' : '';
  if (serverLibrary) {
    // 网页客户端的笔记目录在部署时绑定，不能在这里更换
    storageLocation = row('storage', ['笔记位置'], ['部署时绑定的笔记目录'], document.createElement('span'));
    storageLocation.classList.add('settings-reference-row');
  }
  storagePage.insertBefore(usageGroup, libraryTitle || !syncPanel?.isConnected ? null : syncPanel);
  if (libraryTitle) {
    storagePage.insertBefore(heading('storage', ['本机笔记库']), usageGroup);
    if (storageLocation) usageGroup.append(storageLocation);
  }
  let usageRevision = 0;
  async function updateStorageUsage() {
    const revision = ++usageRevision;
    total.textContent = workflowText('正在统计…');
    let result: StorageUsage | undefined;
    try { result = await libraryStorageUsage(); } catch { /* The sync section remains usable. */ }
    if (disposed || revision !== usageRevision) return;
    const bytes = result ? result.notes + result.files + result.trash : 0;
    total.textContent = result ? formatStorageBytes(bytes) : workflowText('无法读取用量');
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
  const localizeLabels = bindLabels(root);
  let appliedLanguage: string | undefined;
  function updateControls() {
    if (appliedLanguage !== document.documentElement.lang) {
      appliedLanguage = document.documentElement.lang;
      localizeLabels();
      bindLabels(todayFolder)();
      updateDailyTitleExamples();
      refreshMcpLanguage?.();
      recorder?.refreshLanguage();
      accountPanel?.refreshLanguage();
      refreshSyncLanguage?.();
      if (switchConfirmation?.open) bindLabels(switchConfirmation)();
      if (passwordDialog?.open) bindLabels(passwordDialog)();
      const section = selected === 'sync' ? 'storage' : selected;
      if (detailTitle) detailTitle.textContent = sectionTitle(section);
      if (desktopTitle) desktopTitle.textContent = sectionTitle(section);
      if (parentButton && section && settingsParents[section]) parentButton.textContent = sectionTitle(settingsParents[section]);
      if (search?.value && results && !results.hidden) search.dispatchEvent(new Event('input'));
    }
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
  window.addEventListener('ui-language-change', updateControls);
  const nativeError = (event: Event) => error((event as CustomEvent<string>).detail);
  window.addEventListener('settings-error', nativeError);
  updateControls(); if (preferences.error) error(preferences.error);
  if (!mobile) select('general');
  return { select, loaded, destroy() { disposed = true; switchConfirmation?.close('cancel'); passwordDialog?.close(); accountPanel?.destroy(); for (const choice of choices) choice.destroy(); recorder?.destroy(); if (locationListener) window.removeEventListener('focus', locationListener); unsubscribe(); unsubscribeSync(); window.removeEventListener('ui-language-change', updateControls); window.removeEventListener('settings-error', nativeError); window.removeEventListener('focus', refreshOnFocus); root.removeEventListener('keydown', navigateHistory); root.replaceChildren(); } };
}
