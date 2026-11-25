import { mountWebGlass, usesGlisse } from './web-glass-mount';

// 仿 macOS 的系统提示框（NSAlert）：打开时焦点在提示框本身，默认按钮不画焦点描边；
// 回车执行默认按钮，Esc 由 <dialog> 自己关闭。Tab 进到按钮后才显示键盘焦点。

export function alertActions(primary: HTMLButtonElement, ...others: HTMLButtonElement[]) {
  const actions = document.createElement('div');
  actions.className = 'alert-actions';
  primary.classList.add('button', 'primary-button');
  for (const button of others) button.classList.add('button', 'secondary-button');
  actions.append(primary, ...others);
  return actions;
}

export function showAlert(dialog: HTMLDialogElement, primary: HTMLButtonElement) {
  dialog.classList.add('alert-dialog');
  if (usesGlisse(dialog)) {
    dialog.classList.add('glisse-alert');
    const dispose = mountWebGlass(dialog, { tint: 92, blur: 14 });
    dialog.addEventListener('close', dispose, { once: true });
  }
  dialog.tabIndex = -1;
  dialog.addEventListener('keydown', event => {
    if (event.key === 'Enter' && !event.isComposing && event.target === dialog && !primary.disabled) {
      event.preventDefault();
      primary.click();
    }
  });
  if (!dialog.isConnected) document.body.append(dialog);
  dialog.showModal();
  dialog.focus();
}

export function askToConfirm(message: string, action: string, cancelLabel: string) {
  return new Promise<boolean>(resolve => {
    const dialog = document.createElement('dialog');
    dialog.ariaLabel = message;
    const text = document.createElement('p'); text.textContent = message;
    const confirm = document.createElement('button'), cancel = document.createElement('button');
    confirm.textContent = action; confirm.className = 'destructive-button';
    cancel.textContent = cancelLabel;
    confirm.onclick = () => dialog.close('confirm');
    cancel.onclick = () => dialog.close('cancel');
    dialog.append(text, alertActions(confirm, cancel));
    dialog.addEventListener('close', () => {
      dialog.remove();
      resolve(dialog.returnValue === 'confirm');
    }, { once: true });
    showAlert(dialog, confirm);
  });
}
