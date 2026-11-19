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
