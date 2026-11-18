import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { formatStorageBytes } from './library-settings';

export const guestAccountIcon = '<svg viewBox="0 0 16 16" fill="currentColor"><circle cx="8" cy="4.4" r="2.8"/><path d="M1.9 13.5c0-2.7 2.7-4.6 6.1-4.6s6.1 1.9 6.1 4.6c0 .7-.5 1.1-1.2 1.1H3.1c-.7 0-1.2-.4-1.2-1.1Z"/></svg>';

type AccountInfo = {
  profile: { id: string; name: string | null; displayName: string | null; email: string | null; emailVerified: boolean; avatar: string | null };
  subscription: { active: boolean; expiresAt: number | string | null; quotaBytes: number; emailVerified: boolean; emailVerificationRequired: boolean } | null;
  usage: { usedBytes: number; quotaBytes: number } | null;
};

type Redemption = { years: number; redeemedTime: number | string };
type AccountPage = 'account' | 'personal' | 'signIn' | 'storage';
const profileChanged = 'client-profile-changed';
export type CloudSyncAccess = { accountId?: string; signedInAccountId?: string; reason: string };
const dateTime = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' });
function accountDate(timestamp: number | string | null | undefined) {
  if (!timestamp) return '—';
  const date = new Date(typeof timestamp === 'number' ? timestamp * 1000 : timestamp);
  return Number.isFinite(date.getTime()) ? dateTime.format(date) : '—';
}

function accountPhoto(profile: AccountInfo['profile'] | undefined) {
  const source = profile?.avatar;
  if (!source || source.length >= 100_000 || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(source)) return;
  const image = document.createElement('img');
  image.alt = '';
  image.src = source;
  return image;
}

export function sidebarAccount(button: HTMLButtonElement) {
  const avatar = button.querySelector('.sidebar-account-avatar')!;
  const person = avatar.firstElementChild!;
  const label = button.querySelector('.sidebar-account-name')!;
  let loading = false, refreshAgain = false;
  async function refresh() {
    if (loading) { refreshAgain = true; return; }
    loading = true;
    try {
      const profile = (await invoke<AccountInfo | null>('account_status'))?.profile;
      const emailPrefix = profile?.email?.trim().match(/^([^@]+)@[^@]+$/)?.[1];
      const name = emailPrefix || [profile?.displayName, profile?.name].map(value => value?.trim()).find(value => value && !value.includes('@'));
      const words = name?.split(/[\s._-]+/).filter(Boolean) ?? [];
      const first = Array.from(words[0] ?? '')[0] ?? '';
      const last = words.length > 1 ? Array.from(words.at(-1)!)[0] : '';
      const initials = (first + last).toUpperCase();
      label.textContent = name || (profile ? '轻羽账户' : '本地账户');
      button.ariaLabel = `${label.textContent}，设置`;
      avatar.replaceChildren(accountPhoto(profile) ?? (initials || person));
    } catch { /* Keep the last displayed identity until account status can be read. */ }
    finally {
      loading = false;
      if (refreshAgain) { refreshAgain = false; void refresh(); }
    }
  }
  if ('__TAURI_INTERNALS__' in window) void listen(profileChanged, () => void refresh()).catch(() => {});
  return refresh;
}

const errors: Record<string, string> = {
  invalid_input: '输入内容无效，请检查后重试。',
  login_failed: '登录失败。请检查邮箱和密码，或到账号门户完成验证。',
  rate_limited: '尝试次数过多，请稍后重试。',
  network: '无法连接账号服务，请检查网络后重试。',
  response_lost: '未收到账号服务的响应，请重试。',
  temporarily_unavailable: '账号服务暂不可用，请稍后重试。',
  secure_storage: '无法读写应用内的登录会话，请检查应用数据目录的权限后重试。',
  signed_out: '登录已过期，请重新登录。',
  already_signed_in: '账户状态已变化，请重新读取账户。',
  account_changed: '账户已切换，请重新读取账户。',
  email_unverified: '请先在轻羽门户验证邮箱。',
  redemption_failed: '激活码无法兑换，请检查后重试。',
  open_failed: '无法打开账号门户。',
};

function accountError(reason: unknown) {
  if (typeof reason === 'string' && Object.hasOwn(errors, reason)) return errors[reason];
  if (reason && typeof reason === 'object' && 'code' in reason && reason.code === 'redemption_failed') {
    return 'message' in reason && typeof reason.message === 'string' && reason.message.trim()
      ? reason.message : errors.redemption_failed;
  }
  return '账号操作未完成，请重试。';
}

export function mountClientAccount(pages: Record<AccountPage, HTMLElement>, tab: HTMLButtonElement, navigate: (section: AccountPage) => void,
  cloudSync?: (access: CloudSyncAccess) => void) {
  function mount(page: AccountPage, markup: string) {
    const section = document.createElement('div');
    section.className = 'settings-client-account';
    section.innerHTML = markup + '<p class="client-account-status" role="status"></p>';
    pages[page].prepend(section);
    pages[page].classList.add('settings-account-page');
    return section;
  }

  const panel = mount('account', `
    <div class="settings-identity"><span class="settings-avatar" aria-hidden="true">${guestAccountIcon}</span><strong data-account-name>本地账户</strong></div>
    <h2>轻羽账号</h2>
    <div class="client-list">
      <button type="button" class="settings-row settings-disclosure client-sign-in-entry" aria-haspopup="dialog" hidden>
        <span class="settings-label"><span>登录轻羽</span><small>查看账号、套餐及云存储用量。</small></span>
      </button>
      <div data-account-actions hidden>
        <button type="button" class="settings-row settings-disclosure" data-settings-target="personal"><span class="settings-label"><span>个人信息</span></span></button>
        <button type="button" class="settings-row settings-disclosure" data-settings-target="signIn"><span class="settings-label"><span>登录与安全</span></span></button>
        <button type="button" class="settings-row" data-refresh-account><span class="settings-label"><span>刷新账户信息</span></span></button>
        <button type="button" class="settings-row client-sign-out" aria-haspopup="dialog"><span class="settings-label"><span>退出登录</span></span></button>
      </div>
      <button type="button" class="settings-row" data-retry hidden>重新读取账户</button>
    </div>
    <div class="client-list client-storage-link">
      <button type="button" class="settings-row settings-disclosure" data-settings-target="storage"><span class="settings-label"><span>存储与同步</span><small data-account-usage hidden></small></span></button>
    </div>
    <dialog class="client-sign-in" aria-label="登录轻羽">
      <button type="button" class="client-text-button client-sign-in-close" aria-label="关闭登录">×</button>
      <form novalidate>
        <header>
          <img src="/brand/qingyu-app.png" alt="">
          <h2>登录轻羽</h2>
        </header>
        <fieldset>
          <input type="email" name="username" aria-label="邮箱" placeholder="邮箱" autocomplete="username" autocapitalize="none" spellcheck="false" maxlength="320" required>
          <button type="button" class="client-text-button" data-change-email hidden>更改邮箱</button>
          <input type="password" name="password" aria-label="密码" placeholder="密码" autocomplete="current-password" maxlength="4096" hidden>
          <p class="client-account-status" role="status"></p>
          <button type="submit" class="primary-button" disabled>继续</button>
        </fieldset>
        <footer>
          <p>通过轻羽门户管理账号。</p>
          <button type="button" class="client-text-button" data-portal="register">创建账号</button>
          <button type="button" class="client-text-button" data-portal="reset">找回密码</button>
        </footer>
      </form>
    </dialog>`);
  const personal = mount('personal', `
    <div class="client-list" data-account-details hidden>
      <div class="settings-row"><span class="settings-label"><span>头像</span></span><span class="settings-avatar" aria-hidden="true">${guestAccountIcon}</span></div>
      <div class="settings-row"><span class="settings-label"><span>显示名</span></span><span data-display-name></span></div>
      <button type="button" class="settings-row" data-portal="account"><span class="settings-label"><span>在轻羽门户管理个人信息</span></span><span aria-hidden="true">↗</span></button>
    </div>
    <p data-account-required hidden>请先登录轻羽。</p>`);
  const security = mount('signIn', `
    <div class="client-list" data-account-details hidden>
      <div class="settings-row"><span class="settings-label"><span>登录方式</span></span><span>轻羽账号</span></div>
      <div class="settings-row"><span class="settings-label"><span>邮箱</span><small data-profile-email></small></span><span data-email-verification></span></div>
      <button type="button" class="settings-row" data-portal="account"><span class="settings-label"><span>在轻羽门户管理密码</span></span><span aria-hidden="true">↗</span></button>
      <button type="button" class="settings-row client-sign-out" aria-haspopup="dialog" data-logout-all><span class="settings-label"><span>退出所有客户端</span></span></button>
    </div>
    <p data-account-required hidden>请先登录轻羽。</p>`);
  const storage = mount('storage', `
    <h2>套餐与空间</h2>
    <div class="client-list" data-storage-sign-in hidden>
      <button type="button" class="settings-row settings-disclosure" data-settings-target="account"><span class="settings-label"><span>登录轻羽</span><small>登录后查看套餐与用量。</small></span></button>
    </div>
    <button type="button" class="client-text-button" data-retry hidden>重新读取账户</button>
    <div data-account-details hidden>
      <div class="client-list">
        <div class="client-usage">
          <output data-usage></output><progress max="1" value="0" aria-label="云存储用量"></progress>
          <small data-usage-unavailable hidden>用量暂不可用，请刷新。</small>
        </div>
        <div class="settings-row"><span class="settings-label"><span>到期时间</span><small data-subscription></small></span><span data-subscription-expiry></span></div>
        <button type="button" class="settings-row settings-disclosure" data-open-redeem aria-expanded="false"><span class="settings-label"><span>兑换激活码</span><small data-redeem-policy hidden>请先验证邮箱。</small></span></button>
        <form class="client-inline-form client-redeem" hidden>
          <input aria-label="激活码" name="activationCode" placeholder="激活码" autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="4096" required>
          <button type="button" class="client-text-button" data-cancel-redeem>取消</button>
          <button type="submit" class="client-text-button">兑换</button>
        </form>
        <button type="button" class="settings-row" data-portal="verify" hidden><span class="settings-label"><span>验证邮箱</span></span><span aria-hidden="true">↗</span></button>
        <button type="button" class="settings-row" data-refresh-storage><span class="settings-label"><span>刷新套餐与用量</span></span></button>
        <button type="button" class="settings-row" data-portal="purchase"><span class="settings-label"><span>购买或续费</span></span><span aria-hidden="true">↗</span></button>
      </div>
      <h2>兑换记录</h2>
      <ul class="client-list client-redemptions" aria-label="兑换记录"></ul>
      <button type="button" class="client-text-button" data-retry-history hidden>重新读取兑换记录</button>
    </div>`);
  const sections = { account: panel, personal, signIn: security, storage };
  const accountSections = Object.values(sections);
  const signIn = panel.querySelector('dialog')!;
  const openSignIn = panel.querySelector<HTMLButtonElement>('.client-sign-in-entry')!;
  const form = signIn.querySelector('form')!;
  const fields = form.querySelector('fieldset')!;
  const email = form.querySelector<HTMLInputElement>('[name=username]')!;
  const password = form.querySelector<HTMLInputElement>('[name=password]')!;
  const submit = form.querySelector<HTMLButtonElement>('[type=submit]')!;
  const change = form.querySelector<HTMLButtonElement>('[data-change-email]')!;
  const closeSignIn = signIn.querySelector<HTMLButtonElement>('.client-sign-in-close')!;
  const signInMessage = signIn.querySelector<HTMLElement>('[role=status]')!;

  const accountActions = panel.querySelector<HTMLElement>('[data-account-actions]')!;
  const logout = accountActions.querySelector<HTMLButtonElement>('.client-sign-out')!;
  const logoutAll = security.querySelector<HTMLButtonElement>('[data-logout-all]')!;
  const confirmation = document.createElement('dialog');
  confirmation.className = 'client-logout-confirmation';
  confirmation.role = 'alertdialog';
  confirmation.setAttribute('aria-labelledby', 'client-logout-title');
  confirmation.setAttribute('aria-describedby', 'client-logout-description');
  confirmation.innerHTML = `<h2 id="client-logout-title"></h2>
    <p id="client-logout-description"></p><p class="client-account-status" role="status"></p>
    <div class="client-logout-actions">
      <button type="button" data-cancel-logout>取消</button>
      <button type="button" class="client-sign-out" data-confirm-logout>退出登录</button>
    </div>`;
  (panel.closest('.settings') ?? pages.account.parentElement ?? pages.account).append(confirmation);
  const cancelLogout = confirmation.querySelector<HTMLButtonElement>('[data-cancel-logout]')!;
  const confirmLogout = confirmation.querySelector<HTMLButtonElement>('[data-confirm-logout]')!;
  const confirmationStatus = confirmation.querySelector<HTMLElement>('[role=status]')!;
  const tabAvatar = tab.querySelector('.settings-avatar') ?? document.createElement('span');
  if (!tabAvatar.parentElement) {
    tabAvatar.className = 'settings-avatar'; tabAvatar.ariaHidden = 'true';
    const icon = tab.querySelector('svg');
    if (icon) tabAvatar.append(icon);
    tab.prepend(tabAvatar);
  }
  const redeem = storage.querySelector<HTMLButtonElement>('[data-open-redeem]')!;
  const redeemForm = storage.querySelector<HTMLFormElement>('.client-redeem')!;
  const code = redeemForm.querySelector('input')!;
  const historyList = storage.querySelector('ul')!;
  const retryHistory = storage.querySelector<HTMLButtonElement>('[data-retry-history]')!;
  const meter = storage.querySelector('progress')!;
  const viewport = pages.account.closest('.settings-mobile') ? window.visualViewport : undefined;
  let info: AccountInfo | null | undefined;
  let activePage: AccountPage | undefined;
  let logoutTrigger: HTMLButtonElement | undefined;
  let historyAccount: string | undefined;
  let historyRevision = 0;
  let accountRevision = 0;
  let refreshing = false;
  let busy = false, disposed = false;

  function showMessage(text = '', error = false) {
    const container = activePage ? sections[activePage] : panel;
    const target = signIn.open ? signInMessage : confirmation.open ? confirmationStatus : container.querySelector<HTMLElement>(':scope > [role=status]')!;
    target.textContent = text;
    if (error) target.dataset.error = 'true';
    else delete target.dataset.error;
  }

  function updateSubmit() {
    submit.disabled = busy || !(email.readOnly ? password.value : email.value.trim());
  }

  function passwordStep(next: boolean, focus = false) {
    email.readOnly = next;
    email.tabIndex = next ? -1 : 0;
    password.required = next;
    password.value = '';
    password.hidden = !next;
    change.hidden = !next;
    submit.textContent = next ? '登录' : '继续';
    updateSubmit();
    if (focus) (next ? password : email).focus();
  }

  function fitSignIn() {
    if (viewport) signIn.style.setProperty('--client-viewport-height', viewport.height + 'px');
  }

  function resetSignIn() {
    if (signIn.open) return;
    viewport?.removeEventListener('resize', fitSignIn);
    email.value = '';
    passwordStep(false);
    signInMessage.textContent = '';
  }

  openSignIn.onclick = () => {
    showMessage();
    fitSignIn();
    viewport?.addEventListener('resize', fitSignIn);
    signIn.showModal();
    email.focus();
  };
  signIn.addEventListener('cancel', event => {
    event.stopPropagation();
    if (busy) event.preventDefault();
  });
  signIn.addEventListener('keydown', event => {
    if (event.key === 'Escape') event.stopPropagation();
  });
  signIn.onclose = resetSignIn;
  closeSignIn.onclick = () => signIn.close();
  signIn.onclick = event => {
    if (event.target !== signIn || busy) return;
    const { left, right, top, bottom } = signIn.getBoundingClientRect();
    if (event.clientX < left || event.clientX > right || event.clientY < top || event.clientY > bottom) signIn.close();
  };
  form.oninput = () => { showMessage(); updateSubmit(); };


  function setRedeemOpen(open: boolean) {
    redeemForm.hidden = !open;
    redeem.ariaExpanded = String(open);
    code.value = '';
    if (open) code.focus();
  }

  function render() {
    cancelLogout.disabled = confirmLogout.disabled = busy;
    for (const container of accountSections) {
      for (const control of container.querySelectorAll<HTMLButtonElement | HTMLInputElement>('button, input')) control.disabled = busy;
      for (const element of container.querySelectorAll<HTMLElement>('[data-account-details]')) element.hidden = !info;
      const retry = container.querySelector<HTMLElement>('[data-retry]');
      if (retry) retry.hidden = info !== undefined || busy;
    }
    fields.disabled = busy;
    updateSubmit();
    const data = info?.profile;
    const name = data
      ? data.displayName?.trim() || data.name?.trim() || '轻羽账户'
      : '本地账户';
    const tabName = tab.querySelector('strong');
    if (tabName) { tabName.textContent = name; tab.ariaLabel = name; }
    for (const avatar of [tab, panel, personal].flatMap(container => [...container.querySelectorAll('.settings-avatar')])) {
      if (data) avatar.replaceChildren(accountPhoto(data) ?? Array.from(name).slice(0, 2).join(''));
      else avatar.innerHTML = guestAccountIcon;
    }
    panel.querySelector('[data-account-name]')!.textContent = name;
    openSignIn.hidden = info !== null;
    accountActions.hidden = !data;
    for (const container of [personal, security]) container.querySelector<HTMLElement>('[data-account-required]')!.hidden = info !== null;
    storage.querySelector<HTMLElement>('[data-storage-sign-in]')!.hidden = info !== null;
    const plan = info?.subscription;
    const usage = info?.usage;
    const capacity = `${formatStorageBytes(usage?.usedBytes)} / ${formatStorageBytes(usage?.quotaBytes ?? plan?.quotaBytes)}`;
    const accountUsage = panel.querySelector<HTMLElement>('[data-account-usage]')!;
    accountUsage.hidden = !usage;
    accountUsage.textContent = usage ? capacity : '';
    personal.querySelector('[data-display-name]')!.textContent = data ? name : '';
    security.querySelector('[data-profile-email]')!.textContent = data ? data.email || '—' : '';
    security.querySelector('[data-email-verification]')!.textContent = data ? data.emailVerified ? '已验证' : '未验证' : '';
    const verified = plan?.emailVerified ?? data?.emailVerified;
    const needsVerification = Boolean(plan && plan.emailVerificationRequired !== false && !verified);
    cloudSync?.({
      accountId: plan?.active && !needsVerification ? data?.id : undefined,
      signedInAccountId: data?.id,
      reason: !data ? '请登录轻羽' : !plan ? '请刷新套餐与用量'
        : !plan.active ? '套餐未开通或已到期' : needsVerification ? '请先验证邮箱' : '',
    });
    if (!data) return;
    storage.querySelector('[data-usage]')!.textContent = '已用 ' + capacity;
    storage.querySelector<HTMLElement>('[data-usage-unavailable]')!.hidden = !plan?.active || needsVerification || !!usage;
    meter.value = usage && usage.quotaBytes > 0 ? Math.min(1, usage.usedBytes / usage.quotaBytes) : 0;
    storage.querySelector('[data-subscription]')!.textContent = !plan ? '套餐暂不可用，请刷新。'
      : plan.active ? '套餐有效' : '套餐未开通或已到期';
    storage.querySelector('[data-subscription-expiry]')!.textContent = accountDate(plan?.expiresAt);
    storage.querySelector<HTMLElement>('[data-redeem-policy]')!.hidden = !needsVerification;
    storage.querySelector<HTMLElement>('[data-portal=verify]')!.hidden = !needsVerification;
    redeem.disabled = busy || needsVerification;
    if (needsVerification && !redeemForm.hidden) setRedeemOpen(false);
  }

  function invalidateHistory() {
    historyAccount = undefined;
    historyRevision++;
    historyList.replaceChildren();
    retryHistory.hidden = true;
  }

  function setInfo(next: typeof info) {
    const previous = info?.profile, current = next?.profile;
    if (previous?.id !== current?.id) {
      if (confirmation.open) confirmation.close();
      setRedeemOpen(false);
      invalidateHistory();
    }
    if (info !== undefined && (previous?.id !== current?.id || previous?.avatar !== current?.avatar || previous?.email !== current?.email ||
      previous?.displayName !== current?.displayName || previous?.name !== current?.name) && '__TAURI_INTERNALS__' in window) {
      void emit(profileChanged).catch(() => {});
    }
    info = next;
  }

  function historyNote(text: string) {
    const item = document.createElement('li');
    item.className = 'settings-row'; item.textContent = text;
    historyList.replaceChildren(item);
  }

  async function readHistory(force = false) {
    const accountId = info?.profile.id;
    if (!accountId || (!force && historyAccount === accountId)) return;
    historyAccount = accountId;
    const revision = ++historyRevision;
    const hasHistory = historyList.hasChildNodes();
    retryHistory.hidden = true;
    if (!hasHistory) historyNote('正在读取…');
    try {
      const records = await invoke<Redemption[]>('account_redemptions', force ? { accountId, force: true } : { accountId });
      if (disposed || revision !== historyRevision) return;
      if (!records.length) { historyNote('暂无兑换记录'); return; }
      historyList.replaceChildren(...records.map(record => {
        const item = document.createElement('li');
        item.className = 'settings-row';
        const label = document.createElement('span'); label.className = 'settings-label';
        const years = document.createElement('span'); years.textContent = `${record.years} 年`;
        const date = document.createElement('small'); date.textContent = accountDate(record.redeemedTime);
        label.append(years, date); item.append(label);
        return item;
      }));
    } catch (reason) {
      if (disposed || revision !== historyRevision) return;
      if (reason === 'signed_out' || reason === 'account_changed') {
        setInfo(reason === 'signed_out' ? null : undefined);
        render();
        showMessage(accountError(reason), true);
        return;
      }
      if (!hasHistory) historyNote('无法读取兑换记录。');
      retryHistory.hidden = false;
    } finally {
      if (revision === historyRevision) historyAccount = undefined;
    }
  }

  async function run(action: () => Promise<void>, status: string) {
    if (busy || disposed) return;
    accountRevision++;
    busy = true;
    showMessage(status);
    render();
    try { await action(); }
    catch (reason) {
      if (disposed) return;
      if (reason === 'signed_out') setInfo(null);
      if (reason === 'account_changed' || reason === 'already_signed_in') {
        if (signIn.open) signIn.close();
        setInfo(undefined);
      }
      showMessage(accountError(reason), true);
    } finally {
      busy = false;
      if (!disposed) render();
    }
  }

  async function readAccount(force: boolean) {
    const revision = accountRevision;
    const previous = info;
    const next = force ? await invoke<AccountInfo | null>('account_status', { force: true }) : await invoke<AccountInfo | null>('account_status');
    if (disposed || revision !== accountRevision) return;
    setInfo(next);
    render();
    if (activePage === 'storage') void readHistory(force);
    if (force || previous === undefined || previous && !info) showMessage(previous && !info ? errors.signed_out : '');
  }
  const refresh = () => void run(() => readAccount(true), '正在读取账户…');
  const refreshOnFocus = () => {
    if (!activePage || info === null || busy || refreshing) return;
    refreshing = true;
    void readAccount(false).catch(() => { /* Keep the current account view during a background refresh. */ }).finally(() => { refreshing = false; });
  };

  form.onsubmit = event => {
    event.preventDefault();
    if (busy) return;
    if (!email.readOnly) {
      email.value = email.value.trim();
      if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.value)) {
        showMessage('请输入有效的邮箱地址。', true);
        email.focus();
        return;
      }
      showMessage();
      passwordStep(true, true);
      return;
    }
    if (!password.value) return;
    const username = email.value, secret = password.value;
    password.value = '';
    updateSubmit();
    void run(async () => {
      setInfo(await invoke<AccountInfo>('account_login', { username, password: secret }));
      if (disposed) return;
      signIn.close();
      resetSignIn();
      showMessage('已登录。');
    }, '正在登录…');
  };
  change.onclick = () => { showMessage(); passwordStep(false, true); };

  function confirmSignOut(trigger: HTMLButtonElement) {
    if (busy || !info || confirmation.open) return;
    logoutTrigger = trigger;
    const everywhere = trigger === logoutAll;
    confirmation.querySelector('h2')!.textContent = everywhere ? '退出所有客户端？' : '退出登录？';
    confirmation.querySelector('#client-logout-description')!.textContent = everywhere
      ? '所有轻羽客户端都需要重新登录。' : '退出后需要重新登录才能同步。此设备上的笔记会保留。';
    confirmLogout.textContent = everywhere ? '全部退出' : '退出登录';
    confirmationStatus.textContent = '';
    confirmation.showModal();
    cancelLogout.focus();
  }
  logout.onclick = () => confirmSignOut(logout);
  logoutAll.onclick = () => confirmSignOut(logoutAll);
  cancelLogout.onclick = () => confirmation.close();
  confirmation.oncancel = event => {
    event.preventDefault(); event.stopPropagation();
    if (!busy) confirmation.close();
  };
  confirmation.onkeydown = event => { if (event.key === 'Escape') event.stopPropagation(); };
  confirmation.onclose = () => logoutTrigger?.focus({ preventScroll: true });
  confirmLogout.onclick = () => {
    const accountId = info?.profile.id, everywhere = logoutTrigger === logoutAll;
    if (!accountId || !confirmation.open) return;
    void run(async () => {
      const revoked = await invoke<boolean>('account_logout', { accountId, everywhere });
      if (disposed) return;
      setInfo(null);
      showMessage(everywhere ? '已退出所有客户端。'
        : revoked ? '已退出登录。' : '已退出本机账户，服务器会话撤销未确认。');
    }, '正在退出…');
  };

  redeem.onclick = () => setRedeemOpen(redeemForm.hidden);
  redeemForm.querySelector<HTMLButtonElement>('[data-cancel-redeem]')!.onclick = () => { setRedeemOpen(false); redeem.focus(); };
  redeemForm.onsubmit = event => {
    event.preventDefault();
    const accountId = info?.profile.id;
    const activationCode = code.value.trim();
    if (!accountId || busy || redeem.disabled || redeemForm.hidden || !activationCode) return;
    setRedeemOpen(false);
    void run(async () => {
      let years: number;
      try { years = await invoke<number>('account_redeem', { accountId, activationCode }); }
      catch (reason) {
        if (reason === 'response_lost' || reason === 'invalid_response') {
          if (!disposed) {
            void readHistory(true);
            showMessage('兑换结果未确认，请查看兑换记录。', true);
          }
          return;
        }
        throw reason;
      }
      if (disposed) return;
      try {
        const next = await invoke<AccountInfo | null>('account_status', { force: true });
        if (disposed) return;
        setInfo(next);
        showMessage(next?.profile.id === accountId && next.subscription
          ? `已兑换 ${years} 年套餐。` : '兑换成功，请刷新账户查看套餐。');
      } catch {
        if (!disposed) showMessage('兑换成功，请刷新账户查看套餐。');
      }
      if (!disposed) {
        invalidateHistory();
        if (activePage === 'storage') void readHistory();
      }
    }, '正在兑换…');
  };

  retryHistory.onclick = () => void readHistory(true);
  for (const container of accountSections) {
    for (const button of container.querySelectorAll<HTMLButtonElement>('[data-retry], [data-refresh-account], [data-refresh-storage]')) button.onclick = refresh;
    for (const button of container.querySelectorAll<HTMLButtonElement>('[data-settings-target]')) {
      button.onclick = () => navigate(button.dataset.settingsTarget as AccountPage);
    }
    for (const button of container.querySelectorAll<HTMLButtonElement>('[data-portal]')) {
      button.onclick = () => void invoke('account_open_portal', { page: button.dataset.portal }).catch(() => {
        if (!disposed) showMessage(errors.open_failed, true);
      });
    }
  }
  function leave() {
    if (signIn.open) signIn.close();
    resetSignIn();
    setRedeemOpen(false);
    if (confirmation.open) confirmation.close();
  }
  window.addEventListener('focus', refreshOnFocus);
  void run(() => readAccount(false), '正在读取账户…');
  return {
    show(section: string | undefined) {
      const next = section && Object.hasOwn(sections, section) ? section as AccountPage : undefined;
      if (next !== activePage) {
        leave();
        activePage = next;
      }
      if (next === 'storage') void readHistory();
    },
    destroy() { disposed = true; leave(); confirmation.remove(); window.removeEventListener('focus', refreshOnFocus); },
  };
}
