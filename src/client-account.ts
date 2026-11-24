import { callAccount, openPortal } from './account-client';
import { emit, listen } from '@tauri-apps/api/event';
import { formatStorageBytes } from './library-settings';
import { alertActions } from './alert-dialog';
import { markupLabel, bindLabels, localizeStatus, uiText, workflowText, type WorkflowMessage } from './ui-language';

export const guestAccountIcon = '<svg viewBox="0 0 16 16" fill="currentColor"><circle cx="8" cy="4.4" r="2.8"/><path d="M1.9 13.5c0-2.7 2.7-4.6 6.1-4.6s6.1 1.9 6.1 4.6c0 .7-.5 1.1-1.2 1.1H3.1c-.7 0-1.2-.4-1.2-1.1Z"/></svg>';

type AccountInfo = {
  profile: { id: string; name: string | null; displayName: string | null; email: string | null; emailVerified: boolean; avatar: string | null };
  subscription: { active: boolean; paid?: boolean | null; expiresAt: number | string | null; quotaExpiresAt?: number | string | null; quotaBytes: number; emailVerified: boolean; emailVerificationRequired: boolean } | null;
  usage: { usedBytes: number; quotaBytes: number } | null;
};

type Redeemed = { years: number; durationDays?: number | null; quotaBytes?: number | null };
type Redemption = Redeemed & { redeemedTime: number | string };
type AccountPage = 'account' | 'personal' | 'signIn' | 'storage';
const profileChanged = 'client-profile-changed';
export type CloudSyncAccess = { accountId?: string; signedInAccountId?: string; reason: string };
function accountDate(timestamp: number | string | null | undefined) {
  if (!timestamp) return '—';
  const date = new Date(typeof timestamp === 'number' ? timestamp * 1000 : timestamp);
  return Number.isFinite(date.getTime()) ? date.toLocaleString(document.documentElement.lang || 'zh-CN', { dateStyle: 'medium', timeStyle: 'short' }) : '—';
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
  let signedIn = false, displayName: string | undefined;
  function refreshLanguage() {
    label.textContent = displayName || workflowText(signedIn ? '轻羽账户' : '本地账户');
    button.ariaLabel = [label.textContent, uiText('settings.title')].join(document.documentElement.lang === 'zh-CN' ? '，' : ', ');
  }
  async function refresh() {
    if (loading) { refreshAgain = true; return; }
    loading = true;
    try {
      const profile = (await callAccount<AccountInfo | null>('account_status'))?.profile;
      const emailPrefix = profile?.email?.trim().match(/^([^@]+)@[^@]+$/)?.[1];
      const name = emailPrefix || [profile?.displayName, profile?.name].map(value => value?.trim()).find(value => value && !value.includes('@'));
      const words = name?.split(/[\s._-]+/).filter(Boolean) ?? [];
      const first = Array.from(words[0] ?? '')[0] ?? '';
      const last = words.length > 1 ? Array.from(words.at(-1)!)[0] : '';
      const initials = (first + last).toUpperCase();
      signedIn = Boolean(profile);
      displayName = name;
      refreshLanguage();
      avatar.replaceChildren(accountPhoto(profile) ?? (initials || person));
    } catch { /* Keep the last displayed identity until account status can be read. */ }
    finally {
      loading = false;
      if (refreshAgain) { refreshAgain = false; void refresh(); }
    }
  }
  // App 的设置是独立窗口，用 Tauri 事件通知；网页客户端的设置在同一页面，用页面事件
  if ('__TAURI_INTERNALS__' in window) void listen(profileChanged, () => void refresh()).catch(() => {});
  else window.addEventListener(profileChanged, () => void refresh());
  window.addEventListener('ui-language-change', refreshLanguage);
  refreshLanguage();
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
  config_required: '部署时没有设置配置目录（QINGYU_CONFIG_DIR），暂时不能登录轻羽账号。',
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
    <div class="settings-identity"><span class="settings-avatar" aria-hidden="true">${guestAccountIcon}</span><strong data-account-name>${markupLabel('本地账户')}</strong></div>
    <h2 data-i18n="轻羽账号">轻羽账号</h2>
    <div class="client-list">
      <button type="button" class="settings-row settings-disclosure client-sign-in-entry" aria-haspopup="dialog" hidden>
        <span class="settings-label"><span data-i18n="登录轻羽">登录轻羽</span><small data-i18n="查看账号、套餐及云存储用量。">查看账号、套餐及云存储用量。</small></span>
      </button>
      <div data-account-actions hidden>
        <button type="button" class="settings-row settings-disclosure" data-settings-target="personal"><span class="settings-label"><span data-i18n="个人信息">个人信息</span></span></button>
        <button type="button" class="settings-row settings-disclosure" data-settings-target="signIn"><span class="settings-label"><span data-i18n="登录与安全">登录与安全</span></span></button>
        <button type="button" class="settings-row" data-refresh-account><span class="settings-label"><span data-i18n="刷新账户信息">刷新账户信息</span></span></button>
        <button type="button" class="settings-row client-sign-out" aria-haspopup="dialog"><span class="settings-label"><span data-i18n="退出登录">退出登录</span></span></button>
      </div>
      <button type="button" class="settings-row" data-retry hidden data-i18n="重新读取账户">重新读取账户</button>
    </div>
    <div class="client-list client-storage-link">
      <button type="button" class="settings-row settings-disclosure" data-settings-target="storage"><span class="settings-label"><span data-i18n="存储与同步">存储与同步</span><small data-account-usage hidden></small></span></button>
    </div>
    <dialog class="client-sign-in" aria-label="登录轻羽" data-i18n-aria-label="登录轻羽">
      <button type="button" class="client-text-button client-sign-in-close" aria-label="关闭登录" data-i18n-aria-label="关闭登录">×</button>
      <form novalidate>
        <header>
          <img src="/brand/qingyu-app.png" alt="">
          <h2 data-i18n="登录轻羽">登录轻羽</h2>
        </header>
        <fieldset>
          <input type="email" name="username" aria-label="邮箱" data-i18n-aria-label="邮箱" placeholder="邮箱" data-i18n-placeholder="邮箱" autocomplete="username" autocapitalize="none" spellcheck="false" maxlength="320" required>
          <button type="button" class="client-text-button" data-change-email hidden data-i18n="更改邮箱">更改邮箱</button>
          <input type="password" name="password" aria-label="密码" data-i18n-aria-label="密码" placeholder="密码" data-i18n-placeholder="密码" autocomplete="current-password" maxlength="4096" hidden>
          <label class="client-remember-device" hidden><input type="checkbox" name="rememberDevice"> <span data-i18n="记住登录">记住登录</span></label>
          <p class="client-account-status" role="status"></p>
          <button type="submit" class="button primary-button" disabled>${markupLabel('继续')}</button>
        </fieldset>
        <footer>
          <p data-i18n="通过轻羽门户管理账号。">通过轻羽门户管理账号。</p>
          <button type="button" class="client-text-button" data-portal="register" data-i18n="创建账号">创建账号</button>
          <button type="button" class="client-text-button" data-portal="reset" data-i18n="找回密码">找回密码</button>
        </footer>
      </form>
    </dialog>`);
  const personal = mount('personal', `
    <div class="client-list" data-account-details hidden>
      <div class="settings-row"><span class="settings-label"><span data-i18n="头像">头像</span></span><span class="settings-avatar" aria-hidden="true">${guestAccountIcon}</span></div>
      <div class="settings-row"><span class="settings-label"><span data-i18n="显示名">显示名</span></span><span data-display-name></span></div>
      <button type="button" class="settings-row" data-portal="account"><span class="settings-label"><span data-i18n="在轻羽门户管理个人信息">在轻羽门户管理个人信息</span></span><span aria-hidden="true">↗</span></button>
    </div>
    <p data-account-required hidden data-i18n="请先登录轻羽。">请先登录轻羽。</p>`);
  const security = mount('signIn', `
    <div class="client-list" data-account-details hidden>
      <div class="settings-row"><span class="settings-label"><span data-i18n="登录方式">登录方式</span></span><span data-i18n="轻羽账号">轻羽账号</span></div>
      <div class="settings-row"><span class="settings-label"><span data-i18n="邮箱">邮箱</span><small data-profile-email></small></span><span data-email-verification></span></div>
      <button type="button" class="settings-row" data-portal="account"><span class="settings-label"><span data-i18n="在轻羽门户管理密码">在轻羽门户管理密码</span></span><span aria-hidden="true">↗</span></button>
      <button type="button" class="settings-row client-sign-out" aria-haspopup="dialog" data-logout-all><span class="settings-label"><span data-i18n="退出所有客户端">退出所有客户端</span></span></button>
    </div>
    <p data-account-required hidden data-i18n="请先登录轻羽。">请先登录轻羽。</p>`);
  const storage = mount('storage', `
    <h2 data-i18n="套餐与空间">套餐与空间</h2>
    <div class="client-list" data-storage-sign-in hidden>
      <button type="button" class="settings-row settings-disclosure" data-settings-target="account"><span class="settings-label"><span data-i18n="登录轻羽">登录轻羽</span><small data-i18n="登录后查看套餐与用量。">登录后查看套餐与用量。</small></span></button>
    </div>
    <button type="button" class="client-text-button" data-retry hidden data-i18n="重新读取账户">重新读取账户</button>
    <div data-account-details hidden>
      <div class="client-list">
        <div class="client-usage">
          <output data-usage></output><progress max="1" value="0" aria-label="云存储用量" data-i18n-aria-label="云存储用量"></progress>
          <small data-usage-unavailable hidden data-i18n="用量暂不可用，请刷新。">用量暂不可用，请刷新。</small>
        </div>
        <div class="settings-row"><span class="settings-label"><span data-i18n="到期时间">到期时间</span><small data-subscription></small></span><span data-subscription-expiry></span></div>
        <div class="settings-row" data-quota-expiry-row hidden><span class="settings-label" data-i18n="当前容量到期时间">当前容量到期时间</span><span data-quota-expiry></span></div>
        <button type="button" class="settings-row settings-disclosure" data-open-redeem aria-haspopup="dialog"><span class="settings-label"><span data-i18n="兑换激活码">兑换激活码</span><small data-redeem-policy hidden data-i18n="请先验证邮箱。">请先验证邮箱。</small></span></button>
        <button type="button" class="settings-row" data-portal="verify" hidden><span class="settings-label"><span data-i18n="验证邮箱">验证邮箱</span></span><span aria-hidden="true">↗</span></button>
        <button type="button" class="settings-row" data-refresh-storage><span class="settings-label"><span data-i18n="刷新套餐与用量">刷新套餐与用量</span></span></button>
        <button type="button" class="settings-row" data-portal="purchase"><span class="settings-label"><span data-i18n="购买或续费">购买或续费</span></span><span aria-hidden="true">↗</span></button>
      </div>
      <h2 data-i18n="兑换记录">兑换记录</h2>
      <ul class="client-list client-redemptions" aria-label="兑换记录" data-i18n-aria-label="兑换记录"></ul>
      <button type="button" class="client-text-button" data-retry-history hidden data-i18n="重新读取兑换记录">重新读取兑换记录</button>
    </div>`);
  const sections = { account: panel, personal, signIn: security, storage };
  const accountSections = Object.values(sections);
  const signIn = panel.querySelector('dialog')!;
  const openSignIn = panel.querySelector<HTMLButtonElement>('.client-sign-in-entry')!;
  const form = signIn.querySelector('form')!;
  const fields = form.querySelector('fieldset')!;
  const email = form.querySelector<HTMLInputElement>('[name=username]')!;
  const password = form.querySelector<HTMLInputElement>('[name=password]')!;
  const remember = form.querySelector<HTMLInputElement>('[name=rememberDevice]')!;
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
      <button type="button" data-cancel-logout data-i18n="取消">取消</button>
      <button type="button" class="client-sign-out" data-confirm-logout>${markupLabel('退出登录')}</button>
    </div>`;
  const redeemDialog = document.createElement('dialog');
  redeemDialog.className = 'alert-dialog';
  redeemDialog.setAttribute('aria-labelledby', 'client-redeem-title');
  redeemDialog.innerHTML = `<form class="client-redeem">
    <h2 id="client-redeem-title" data-i18n="兑换激活码">兑换激活码</h2>
    <input aria-label="激活码" data-i18n-aria-label="激活码" name="activationCode" placeholder="激活码" data-i18n-placeholder="激活码" autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="4096" required autofocus>
    <button type="button" data-cancel-redeem data-i18n="取消">取消</button>
    <button type="submit" data-i18n="兑换">兑换</button>
  </form>`;
  (panel.closest('.settings') ?? pages.account.parentElement ?? pages.account).append(confirmation, redeemDialog);
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
  const redeemForm = redeemDialog.querySelector('form')!;
  const code = redeemForm.querySelector('input')!;
  const [cancelRedeem, submitRedeem] = redeemForm.querySelectorAll('button');
  redeemForm.append(alertActions(submitRedeem!, cancelRedeem!));
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
  const applyLabels = [...accountSections, confirmation, redeemDialog].map(bindLabels);
  const statusCopy = new WeakMap<Element, string>();

  function showMessage(text = '', error = false) {
    const container = activePage ? sections[activePage] : panel;
    const target = signIn.open ? signInMessage : confirmation.open ? confirmationStatus : container.querySelector<HTMLElement>(':scope > [role=status]')!;
    statusCopy.set(target, text);
    target.textContent = localizeStatus(text);
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
    remember.closest('label')!.hidden = !next;
    change.hidden = !next;
    submit.textContent = workflowText(next ? '登录' : '继续');
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
    remember.checked = false;
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


  function closeRedeem() {
    if (redeemDialog.open) redeemDialog.close();
  }

  function render() {
    cancelLogout.disabled = confirmLogout.disabled = busy;
    code.disabled = cancelRedeem!.disabled = submitRedeem!.disabled = busy;
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
      ? data.displayName?.trim() || data.name?.trim() || workflowText('轻羽账户')
      : workflowText('本地账户');
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
    const quota = plan?.quotaBytes ?? usage?.quotaBytes;
    const capacity = `${formatStorageBytes(usage?.usedBytes)} / ${formatStorageBytes(quota)}`;
    const accountUsage = panel.querySelector<HTMLElement>('[data-account-usage]')!;
    accountUsage.hidden = !usage;
    accountUsage.textContent = usage ? capacity : '';
    personal.querySelector('[data-display-name]')!.textContent = data ? name : '';
    security.querySelector('[data-profile-email]')!.textContent = data ? data.email || '—' : '';
    security.querySelector('[data-email-verification]')!.textContent = data ? workflowText(data.emailVerified ? '已验证' : '未验证') : '';
    const verified = plan?.emailVerified ?? data?.emailVerified;
    const needsVerification = Boolean(plan && plan.emailVerificationRequired !== false && !verified);
    const canRead = !!plan && (plan.active || typeof plan.paid === 'boolean') && !needsVerification;
    cloudSync?.({
      accountId: canRead ? data?.id : undefined,
      signedInAccountId: data?.id,
      reason: localizeStatus(!data ? '请登录轻羽' : !plan ? '请刷新套餐与用量'
        : needsVerification ? '请先验证邮箱' : !plan.active
          ? (canRead ? '轻羽云存储写入权益已到期，续费后可继续上传' : '套餐未开通或已到期') : ''),
    });
    if (!data) return;
    storage.querySelector('[data-usage]')!.textContent = workflowText('已用 {capacity}').replace('{capacity}', () => capacity);
    storage.querySelector<HTMLElement>('[data-usage-unavailable]')!.hidden = !canRead || !!usage;
    meter.value = usage && quota && quota > 0 ? Math.min(1, usage.usedBytes / quota) : 0;
    storage.querySelector('[data-subscription]')!.textContent = workflowText(!plan ? '套餐暂不可用，请刷新。'
      : plan.active ? (plan.paid === false ? '免费套餐' : '套餐有效') : '套餐未开通或已到期');
    storage.querySelector('[data-subscription-expiry]')!.textContent = accountDate(plan?.expiresAt);
    const quotaExpiry = accountDate(plan?.quotaExpiresAt);
    storage.querySelector<HTMLElement>('[data-quota-expiry-row]')!.hidden = quotaExpiry === '—' || quotaExpiry === accountDate(plan?.expiresAt);
    storage.querySelector('[data-quota-expiry]')!.textContent = quotaExpiry;
    storage.querySelector<HTMLElement>('[data-redeem-policy]')!.hidden = !needsVerification;
    storage.querySelector<HTMLElement>('[data-portal=verify]')!.hidden = !needsVerification;
    redeem.disabled = busy || needsVerification;
    if (needsVerification) closeRedeem();
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
      closeRedeem();
      invalidateHistory();
    }
    if (info !== undefined && (previous?.id !== current?.id || previous?.avatar !== current?.avatar || previous?.email !== current?.email ||
      previous?.displayName !== current?.displayName || previous?.name !== current?.name) && '__TAURI_INTERNALS__' in window) {
      if ('__TAURI_INTERNALS__' in window) void emit(profileChanged).catch(() => {});
      else dispatchEvent(new Event(profileChanged));
    }
    info = next;
  }

  function historyNote(text: WorkflowMessage) {
    const item = document.createElement('li');
    item.className = 'settings-row'; item.textContent = workflowText(text);
    statusCopy.set(item, text);
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
      const records = await callAccount<Redemption[]>('account_redemptions', force ? { accountId, force: true } : { accountId });
      if (disposed || revision !== historyRevision) return;
      if (!records.length) { historyNote('暂无兑换记录'); return; }
      historyList.replaceChildren(...records.map(record => {
        const item = document.createElement('li');
        item.className = 'settings-row';
        const label = document.createElement('span'); label.className = 'settings-label';
        const years = document.createElement('span'); years.dataset.years = String(record.years);
        years.textContent = record.durationDays == null ? workflowText('{years} 年').replace('{years}', String(record.years))
          : workflowText('{days} 天').replace('{days}', String(record.durationDays));
        if (record.quotaBytes != null) years.textContent += ` · ${formatStorageBytes(record.quotaBytes)}`;
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
    const next = force ? await callAccount<AccountInfo | null>('account_status', { force: true }) : await callAccount<AccountInfo | null>('account_status');
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
    const username = email.value, secret = password.value, rememberDevice = remember.checked;
    password.value = '';
    updateSubmit();
    void run(async () => {
      setInfo(await callAccount<AccountInfo>('account_login', { username, password: secret, rememberDevice }));
      if (disposed) return;
      signIn.close();
      resetSignIn();
      showMessage('已登录。');
    }, '正在登录…');
  };
  change.onclick = () => { showMessage(); passwordStep(false, true); };

  function updateLogoutLabels(everywhere: boolean) {
    confirmation.querySelector('h2')!.textContent = workflowText(everywhere ? '退出所有客户端？' : '退出登录？');
    confirmation.querySelector('#client-logout-description')!.textContent = workflowText(everywhere
      ? '所有轻羽客户端都需要重新登录。' : '退出后需要重新登录才能同步。此设备上的笔记会保留。');
    confirmLogout.textContent = workflowText(everywhere ? '全部退出' : '退出登录');
  }
  function confirmSignOut(trigger: HTMLButtonElement) {
    if (busy || !info || confirmation.open) return;
    logoutTrigger = trigger;
    updateLogoutLabels(trigger === logoutAll);
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
      const revoked = await callAccount<boolean>('account_logout', { accountId, everywhere });
      if (disposed) return;
      setInfo(null);
      showMessage(everywhere ? '已退出所有客户端。'
        : revoked ? '已退出登录。' : '已退出本机账户，服务器会话撤销未确认。');
    }, '正在退出…');
  };

  redeem.onclick = () => { redeemDialog.showModal(); code.focus(); };
  cancelRedeem!.onclick = closeRedeem;
  redeemDialog.onkeydown = event => { if (event.key === 'Escape') event.stopPropagation(); };
  redeemDialog.onclose = () => {
    if (redeemDialog.open) return;
    code.value = '';
    if (!disposed && activePage === 'storage') redeem.focus({ preventScroll: true });
  };
  redeemForm.onsubmit = event => {
    event.preventDefault();
    const accountId = info?.profile.id;
    const activationCode = code.value.trim();
    if (!accountId || busy || redeem.disabled || !redeemDialog.open || !activationCode) return;
    closeRedeem();
    void run(async () => {
      let redeemed: Redeemed | number;
      try { redeemed = await callAccount<Redeemed | number>('account_redeem', { accountId, activationCode }); }
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
        const next = await callAccount<AccountInfo | null>('account_status', { force: true });
        if (disposed) return;
        setInfo(next);
        const plan = typeof redeemed === 'number' ? { years: redeemed } : redeemed;
        showMessage(next?.profile.id === accountId && next.subscription
          ? plan.durationDays == null ? workflowText('已兑换 {years} 年套餐。').replace('{years}', String(plan.years))
            : workflowText('已兑换 {days} 天套餐。').replace('{days}', String(plan.durationDays))
          : '兑换成功，请刷新账户查看套餐。');
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
      button.onclick = () => void openPortal(button.dataset.portal!).catch(() => {
        if (!disposed) showMessage(errors.open_failed, true);
      });
    }
  }
  function leave() {
    if (signIn.open) signIn.close();
    resetSignIn();
    closeRedeem();
    if (confirmation.open) confirmation.close();
  }
  window.addEventListener('focus', refreshOnFocus);
  for (const apply of applyLabels) apply();
  void run(() => readAccount(false), '正在读取账户…');
  return {
    refreshLanguage() {
      for (const apply of applyLabels) apply();
      submit.textContent = workflowText(email.readOnly ? '登录' : '继续');
      for (const container of [...accountSections, confirmation]) {
        for (const label of container.querySelectorAll('[role=status], .client-redemptions li')) {
          const source = statusCopy.get(label);
          if (source && label.textContent) label.textContent = localizeStatus(source);
        }
      }
      for (const label of historyList.querySelectorAll<HTMLElement>('[data-years]')) {
        label.textContent = workflowText('{years} 年').replace('{years}', label.dataset.years!);
      }
      if (confirmation.open) updateLogoutLabels(logoutTrigger === logoutAll);
      render();
    },
    show(section: string | undefined) {
      const next = section && Object.hasOwn(sections, section) ? section as AccountPage : undefined;
      if (next !== activePage) {
        leave();
        activePage = next;
      }
      if (next === 'storage') void readHistory();
    },
    destroy() { disposed = true; leave(); confirmation.remove(); redeemDialog.remove(); window.removeEventListener('focus', refreshOnFocus); },
  };
}
