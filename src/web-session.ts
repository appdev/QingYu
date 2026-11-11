const enabled = () => document.querySelector('meta[name="qingyu-web-auth"][content="enabled"]') !== null;

async function validSession() {
  const response = await fetch('/auth/session', { credentials: 'same-origin', cache: 'no-store' });
  if (response.status === 204) return true;
  if (response.status === 401) return false;
  throw new Error('暂时无法验证登录状态，请稍后重试');
}

export async function requireWebSession() {
  if (!enabled()) return;
  try {
    if (await validSession()) return;
    location.replace('/login.html?expired=1');
    // Stop initialization while navigating, before browser drafts are displayed.
    await new Promise<never>(() => {});
  } catch (reason) {
    document.body.textContent = '无法验证登录状态，请检查网络后刷新页面。';
    throw reason;
  }
}

export function watchWebSession(beforeLeave: (expired: boolean) => Promise<boolean>, error: (message: string) => void) {
  if (!enabled()) return;
  const logout = document.querySelector<HTMLButtonElement>('#logout-button')!;
  logout.hidden = false;
  let leaving = false;
  async function leave(expired: boolean) {
    if (leaving) return;
    leaving = true;
    try {
      if (!await beforeLeave(expired)) return;
      if (!expired) {
        const response = await fetch('/auth/logout', { method: 'POST', credentials: 'same-origin' });
        if (response.status !== 204) throw new Error('退出登录失败，请稍后重试');
      }
      location.replace(expired ? '/login.html?expired=1' : '/login.html');
    } catch (reason) { error(reason instanceof Error ? reason.message : '无法连接服务，请稍后重试'); }
    finally { leaving = false; }
  }
  const check = async () => {
    try { if (!await validSession()) await leave(true); }
    catch { error('暂时无法验证登录状态，本机草稿仍保留'); }
  };
  logout.onclick = () => { void leave(false); };
  window.addEventListener('focus', check);
  window.addEventListener('pageshow', check);
  setInterval(check, 60_000);
}
