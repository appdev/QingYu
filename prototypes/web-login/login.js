// Design preview only: the deployment-configured password is not sent or stored.
const form = document.querySelector('#login-form');
const password = document.querySelector('#password');
const visibility = document.querySelector('#password-visibility');
const submit = document.querySelector('#login-button');
const submitLabel = document.querySelector('#login-button-label');
const message = document.querySelector('#form-message');
const stateButtons = [...document.querySelectorAll('[data-state]')];
const passwordHelp = document.querySelector('#password-help');

function clearErrors() {
  password.removeAttribute('aria-invalid');
  passwordHelp.textContent = '';
  message.textContent = '';
  delete message.dataset.kind;
}

function showState(state) {
  if (!stateButtons.some(button => button.dataset.state === state)) state = 'default';
  clearErrors();
  submit.disabled = state === 'loading';
  submit.setAttribute('aria-busy', String(state === 'loading'));
  submitLabel.textContent = state === 'loading' ? '正在验证…' : '进入笔记库';
  document.querySelector('#session-notice').hidden = state !== 'expired';
  if (state === 'error') {
    password.setAttribute('aria-invalid', 'true');
    message.textContent = '访问密码不正确，请检查后重试。';
    message.dataset.kind = 'error';
  }
  for (const button of stateButtons) button.setAttribute('aria-pressed', String(button.dataset.state === state));
  const url = new URL(location.href);
  if (state === 'default') url.searchParams.delete('state');
  else url.searchParams.set('state', state);
  history.replaceState(null, '', url);
}

for (const button of stateButtons) button.addEventListener('click', () => showState(button.dataset.state));
visibility.addEventListener('click', () => {
  const showing = password.type === 'password';
  password.type = showing ? 'text' : 'password';
  visibility.setAttribute('aria-pressed', String(showing));
  visibility.setAttribute('aria-label', showing ? '隐藏密码' : '显示密码');
});

function validatePassword() {
  const empty = !password.value;
  password.setAttribute('aria-invalid', String(empty));
  passwordHelp.textContent = empty ? '请输入访问密码。' : '';
  return !empty;
}
password.addEventListener('blur', validatePassword);
password.addEventListener('input', () => {
  if (password.hasAttribute('aria-invalid')) validatePassword();
  message.textContent = '';
  delete message.dataset.kind;
});
form.addEventListener('submit', event => {
  event.preventDefault();
  if (submit.disabled) return;
  clearErrors();
  if (!validatePassword()) { password.focus(); return; }
  message.textContent = '这是设计预览，尚未连接验证服务。密码没有发送或保存。';
});
showState(new URLSearchParams(location.search).get('state') ?? 'default');
