// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { mountClientAccount, sidebarAccount } from '../src/client-account';
import { parsePreferences } from '../src/preferences';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn().mockResolvedValue(undefined), listen: vi.fn().mockResolvedValue(() => {}) }));
beforeEach(() => {
  vi.mocked(invoke).mockReset();
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
    close: { configurable: true, value(this: HTMLDialogElement) {
      this.open = false; this.dispatchEvent(new Event('close'));
    } },
  });
});

function accountPages() {
  const root = document.createElement('main');
  const pages = { account: document.createElement('section'), personal: document.createElement('section'),
    signIn: document.createElement('section'), storage: document.createElement('section') };
  root.append(...Object.values(pages));
  document.body.append(root);
  return { root, pages };
}

it('submits only after the password step and updates or clears the account card', async () => {
  const ipc = vi.mocked(invoke);
  ipc.mockResolvedValueOnce(null).mockResolvedValueOnce(undefined)
    .mockRejectedValueOnce('login_failed').mockResolvedValueOnce({
    profile: { id: 'id', name: 'Fixture', displayName: '测试账号', email: 'qa@example.invalid', emailVerified: false, avatar: 'data:image/png;base64,aW1hZ2U=' },
    subscription: { active: true, expiresAt: null, emailVerified: false },
  }).mockResolvedValueOnce(undefined).mockResolvedValueOnce(true);
  const { root, pages } = accountPages();
  const page = pages.account;
  const tab = document.createElement('button'); tab.innerHTML = '<svg></svg><strong>本地账户</strong>';
  root.prepend(tab);
  const navigate = vi.fn();
  const controller = mountClientAccount(pages, tab, navigate);
  controller.show('account');
  try {
    const dialog = page.querySelector('dialog')!;
    const entry = page.querySelector<HTMLButtonElement>('.client-sign-in-entry')!;
    const form = dialog.querySelector('form')!;
    const email = form.querySelector<HTMLInputElement>('[name=username]')!;
    const password = form.querySelector<HTMLInputElement>('[name=password]')!;
    const submit = () => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(entry.hidden).toBe(false));
    entry.click();
    expect(dialog.open).toBe(true);
    expect(document.activeElement).toBe(email);
    expect(dialog.querySelector('[data-portal=reset]')?.textContent).toBe('找回密码');
    dialog.querySelector<HTMLButtonElement>('[data-portal=register]')!.click();
    expect(ipc).toHaveBeenLastCalledWith('account_open_portal', { page: 'register' });
    email.value = 'qa@example'; submit();
    expect(dialog.querySelector('[role=status]')?.textContent).toContain('有效的邮箱');
    email.value = 'qa@example.invalid'; submit();
    expect(email.readOnly).toBe(true);
    expect(document.activeElement).toBe(password);
    expect(ipc).toHaveBeenCalledTimes(2);
    password.value = 'incorrect'; submit();
    expect(password.value).toBe('');
    await vi.waitFor(() => expect(dialog.querySelector('[role=status]')?.textContent).toContain('登录失败'));
    password.value = 'unsaved'; controller.show(undefined);
    expect(password.value).toBe(''); expect(email.readOnly).toBe(false);
    expect(dialog.open).toBe(false);
    controller.show('account');
    entry.click(); email.value = 'qa@example.invalid'; submit(); password.value = 'fixture-value'; submit();
    await vi.waitFor(() => expect(tab.ariaLabel).toBe('测试账号'));
    expect(ipc).toHaveBeenCalledWith('account_login', { username: 'qa@example.invalid', password: 'fixture-value' });
    expect(dialog.open).toBe(false);
    expect(entry.hidden).toBe(true);
    expect(pages.storage.querySelector('[data-subscription]')?.textContent).toBe('套餐有效');
    expect((pages.storage.querySelector('[data-portal=verify]') as HTMLElement).hidden).toBe(false);
    const purchase = pages.storage.querySelector<HTMLButtonElement>('[data-portal=purchase]')!;
    expect(purchase.textContent).toBe('购买或续费↗');
    expect(purchase.disabled).toBe(false);
    purchase.click();
    expect(ipc).toHaveBeenLastCalledWith('account_open_portal', { page: 'purchase' });
    expect(pages.storage.querySelector<HTMLButtonElement>('[data-open-redeem]')?.disabled).toBe(true);
    expect(pages.personal.querySelector('[data-display-name]')?.textContent).toBe('测试账号');
    const avatar = page.querySelector<HTMLImageElement>('.settings-identity img')?.src;
    expect(avatar).toBe('data:image/png;base64,aW1hZ2U=');
    expect(tab.querySelector<HTMLImageElement>('.settings-avatar img')?.src).toBe(avatar);
    expect(pages.personal.querySelector<HTMLImageElement>('.settings-avatar img')?.src).toBe(avatar);
    expect(pages.signIn.querySelector('[data-profile-email]')?.textContent).toBe('qa@example.invalid');
    page.querySelector<HTMLButtonElement>('[data-settings-target=signIn]')!.click();
    expect(navigate).toHaveBeenCalledWith('signIn');
    const signOut = page.querySelector<HTMLButtonElement>('.client-sign-out')!;
    const alert = root.querySelector<HTMLDialogElement>('[role=alertdialog]')!;
    const cancel = alert.querySelector<HTMLButtonElement>('[data-cancel-logout]')!;
    signOut.click();
    expect(alert.open).toBe(true);
    expect(document.activeElement).toBe(cancel);
    expect(alert.textContent).toContain('此设备上的笔记会保留');
    cancel.click();
    expect(alert.open).toBe(false);
    expect(document.activeElement).toBe(signOut);
    expect(ipc.mock.calls.some(([name]) => name === 'account_logout')).toBe(false);
    signOut.click();
    alert.dispatchEvent(new Event('cancel', { cancelable: true }));
    expect(alert.open).toBe(false);
    signOut.click();
    alert.querySelector<HTMLButtonElement>('[data-confirm-logout]')!.click();
    await vi.waitFor(() => expect(tab.ariaLabel).toBe('本地账户'));
    expect(entry.hidden).toBe(false);
    expect(ipc).toHaveBeenLastCalledWith('account_logout', { accountId: 'id', everywhere: false });
    expect(alert.open).toBe(false);
    expect(tab.querySelector('.settings-avatar svg')).not.toBeNull();
    expect(pages.signIn.querySelector('[data-profile-email]')?.textContent).toBe('');
  } finally { controller.destroy(); root.remove(); }
});

it('ignores old local names and omits the signed-out display name editor', async () => {
  const parsed = parsePreferences({ version: 1, appearance: 'dark', profileName: '旧名称' });
  expect(parsed.appearance).toBe('dark');
  expect(parsed).not.toHaveProperty('profileName');
  vi.mocked(invoke).mockResolvedValueOnce(null);
  const { root, pages } = accountPages();
  const controller = mountClientAccount(pages, document.createElement('button'), vi.fn());
  try {
    const signIn = pages.account.querySelector<HTMLButtonElement>('.client-sign-in-entry')!;
    await vi.waitFor(() => expect(signIn.hidden).toBe(false));
    expect(pages.account.querySelector('[data-account-name]')?.textContent).toBe('本地账户');
    expect(pages.account.textContent).not.toContain('显示名');
  } finally { controller.destroy(); root.remove(); }
});

it('refreshes the plan and unavailable usage, redeems, and confirms all-client logout', async () => {
  const ipc = vi.mocked(invoke);
  const account = {
    profile: { id: 'bound-id', name: 'Fixture', emailVerified: false },
    subscription: null,
    usage: null,
  };
  const activated = { ...account, subscription: {
    active: true, expiresAt: '2027-11-17T00:00:00Z', quotaBytes: 10_000_000_000,
    emailVerified: false, emailVerificationRequired: false,
  } };
  const records = [{ years: 1, redeemedTime: 1_800_000_000 }];
  ipc.mockResolvedValueOnce(account)
    .mockResolvedValueOnce([])
    .mockRejectedValueOnce({ code: 'redemption_failed', message: '该激活码已使用。' })
    .mockResolvedValueOnce(1)
    .mockResolvedValueOnce(activated)
    .mockResolvedValueOnce(records)
    .mockResolvedValueOnce({ ...activated, usage: { usedBytes: 512, quotaBytes: 10_000_000_000 } })
    .mockResolvedValueOnce(records)
    .mockRejectedValueOnce('network')
    .mockResolvedValueOnce(true);
  const { root, pages } = accountPages();
  const controller = mountClientAccount(pages, document.createElement('button'), vi.fn());
  const button = (selector: string) => root.querySelector<HTMLButtonElement>(selector)!;
  try {
    controller.show('storage');
    const form = pages.storage.querySelector<HTMLFormElement>('.client-redeem')!;
    const code = form.querySelector('input')!;
    const redeem = () => {
      button('[data-open-redeem]').click();
      code.value = ' fixture-code ';
      form.dispatchEvent(new Event('submit', { cancelable: true }));
    };
    await vi.waitFor(() => expect(pages.storage.querySelector('.client-redemptions li')?.textContent).toBe('暂无兑换记录'));
    expect(button('[data-portal=verify]').hidden).toBe(true);
    redeem();
    const status = pages.storage.querySelector('.settings-client-account > [role=status]');
    await vi.waitFor(() => expect(status?.textContent).toBe('该激活码已使用。'));
    expect(code.value).toBe('');
    redeem();
    await vi.waitFor(() => expect(status?.textContent).toContain('已兑换 1 年'));
    expect(ipc).toHaveBeenCalledWith('account_redeem', { accountId: 'bound-id', activationCode: 'fixture-code' });
    expect(pages.storage.querySelector('[data-subscription-expiry]')?.textContent).toContain('2027');
    expect(pages.storage.querySelector('[data-usage]')?.textContent).toBe('已用 — / 10 GB');
    expect(pages.storage.querySelector<HTMLElement>('[data-usage-unavailable]')?.hidden).toBe(false);
    button('[data-refresh-storage]').click();
    await vi.waitFor(() => expect(pages.storage.querySelector('[data-usage]')?.textContent).toBe('已用 512 B / 10 GB'));
    expect(pages.storage.querySelector<HTMLElement>('[data-usage-unavailable]')?.hidden).toBe(true);
    expect(pages.storage.querySelector('progress')?.value).toBeCloseTo(512 / 10_000_000_000, 12);
    await vi.waitFor(() => expect(pages.storage.querySelector('.client-redemptions li')?.textContent).toContain('1 年'));
    controller.show('signIn');
    button('[data-logout-all]').click();
    expect(ipc).toHaveBeenLastCalledWith('account_redemptions', { accountId: 'bound-id', force: true });
    button('[data-confirm-logout]').click();
    const securityStatus = pages.signIn.querySelector('.settings-client-account > [role=status]');
    const alert = root.querySelector<HTMLDialogElement>('[role=alertdialog]')!;
    expect(alert).toBeTruthy();
    await vi.waitFor(() => expect(alert.querySelector('[role=status]')?.textContent).toContain('无法连接'));
    expect(alert.open).toBe(true);
    expect(button('[data-confirm-logout]').disabled).toBe(false);
    button('[data-confirm-logout]').click();
    await vi.waitFor(() => expect(securityStatus?.textContent).toBe('已退出所有客户端。'));
    expect(ipc).toHaveBeenLastCalledWith('account_logout', { accountId: 'bound-id', everywhere: true });
    expect(pages.storage.querySelector('.client-redemptions')?.textContent).toBe('');
  } finally { controller.destroy(); root.remove(); }
});

it('shows the email prefix in the sidebar, keeps it during a failed refresh and clears it on sign-out', async () => {
  const ipc = vi.mocked(invoke);
  const profile = { name: 'Mira.Fox', displayName: 'Fixture', email: ' mira.fox+notes@example.invalid ', avatar: null };
  const photo = 'data:image/png;base64,aW1hZ2U=';
  ipc.mockResolvedValueOnce({ profile })
    .mockRejectedValueOnce('network')
    .mockResolvedValueOnce({ profile: { ...profile, avatar: photo } })
    .mockResolvedValueOnce(null);
  const button = document.createElement('button');
  button.innerHTML = '<span class="sidebar-account-avatar"><svg></svg></span><span class="sidebar-account-name"></span>';
  const refresh = sidebarAccount(button);
  const avatar = button.querySelector('.sidebar-account-avatar')!;

  await refresh();
  expect(button.ariaLabel).toBe('mira.fox+notes，设置');
  expect(button.querySelector('.sidebar-account-name')?.textContent).toBe('mira.fox+notes');
  expect(avatar.textContent).toBe('MF');
  await refresh();
  expect(button.ariaLabel).toBe('mira.fox+notes，设置');
  await refresh();
  expect(avatar.querySelector('img')?.getAttribute('src')).toBe(photo);
  await refresh();
  expect(button.ariaLabel).toBe('本地账户，设置');
  expect(avatar.firstElementChild?.localName).toBe('svg');
});

it('refreshes both sidebars from the account snapshot after an avatar changes', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  vi.mocked(emit).mockClear();
  vi.mocked(listen).mockClear();
  const oldAvatar = 'data:image/png;base64,b2xk';
  const newAvatar = 'data:image/png;base64,bmV3';
  const profile = (avatar: string) => ({ profile: { id: 'one', name: 'Fixture', avatar } });
  vi.mocked(invoke).mockResolvedValueOnce(profile(oldAvatar)).mockResolvedValueOnce(profile(oldAvatar))
    .mockResolvedValueOnce(profile(newAvatar)).mockResolvedValueOnce(profile(newAvatar));
  const { root, pages } = accountPages();
  const tab = root.appendChild(document.createElement('button'));
  const sidebar = root.appendChild(document.createElement('button'));
  sidebar.innerHTML = '<span class="sidebar-account-avatar"><svg></svg></span><span class="sidebar-account-name"></span>';
  const controller = mountClientAccount(pages, tab, vi.fn());
  const refresh = sidebarAccount(sidebar);
  try {
    controller.show('account');
    await refresh();
    await vi.waitFor(() => expect(tab.querySelector('img')?.src).toBe(oldAvatar));
    pages.account.querySelector<HTMLButtonElement>('[data-refresh-account]')!.click();
    await vi.waitFor(() => expect(tab.querySelector('img')?.src).toBe(newAvatar));
    const [event, listener] = vi.mocked(listen).mock.calls[0];
    expect(emit).toHaveBeenCalledWith(event);
    listener({ event, id: 0, payload: undefined });
    await vi.waitFor(() => expect(sidebar.querySelector('img')?.src).toBe(newAvatar));
  } finally { controller.destroy(); root.remove(); vi.unstubAllGlobals(); }
});
