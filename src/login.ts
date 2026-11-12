import '../prototypes/web-login/login.css';
import './preferences';

const form = document.querySelector<HTMLFormElement>('#login-form')!;
const password = document.querySelector<HTMLInputElement>('#password')!;
const visibility = document.querySelector<HTMLButtonElement>('#password-visibility')!;
const submit = document.querySelector<HTMLButtonElement>('#login-button')!;
const label = document.querySelector<HTMLElement>('#login-button-label')!;
const message = document.querySelector<HTMLElement>('#form-message')!;
const help = document.querySelector<HTMLElement>('#password-help')!;
document.querySelector<HTMLElement>('#session-notice')!.hidden = new URLSearchParams(location.search).get('expired') !== '1';

visibility.onclick = () => {
  const showing = password.type === 'password';
  password.type = showing ? 'text' : 'password';
  visibility.setAttribute('aria-pressed', String(showing));
  visibility.setAttribute('aria-label', showing ? '隐藏密码' : '显示密码');
};
password.oninput = () => {
  password.removeAttribute('aria-invalid'); help.textContent = ''; message.textContent = '';
};
form.onsubmit = async event => {
  event.preventDefault();
  if (submit.disabled) return;
  message.textContent = ''; help.textContent = '';
  if (!password.value) {
    password.setAttribute('aria-invalid', 'true'); help.textContent = '请输入访问密码。'; password.focus(); return;
  }
  submit.disabled = true; submit.setAttribute('aria-busy', 'true'); label.textContent = '正在验证…';
  try {
    const response = await fetch('/auth/login', {
      method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ password: password.value }),
    });
    if (response.status === 204) { password.value = ''; location.replace('/'); return; }
    message.dataset.kind = 'error';
    if (response.status === 401) {
      password.setAttribute('aria-invalid', 'true'); message.textContent = '访问密码不正确，请检查后重试。'; password.focus();
    } else if (response.status === 429) message.textContent = '尝试次数过多，请一分钟后重试。';
    else message.textContent = '暂时无法登录，请稍后重试。';
  } catch {
    message.dataset.kind = 'error'; message.textContent = '无法连接服务，请检查网络后重试。';
  } finally {
    submit.disabled = false; submit.setAttribute('aria-busy', 'false'); label.textContent = '进入笔记库';
  }
};
