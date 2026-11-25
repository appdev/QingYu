import './context-sheet.css';

export function showContextSheet(preview: HTMLElement, options: {
  label: string;
  anchor?: HTMLElement;
  mountMenu(dialog: HTMLDialogElement): void;
  close(): void;
}) {
  const dialog = document.createElement('dialog');
  dialog.className = 'context-sheet';
  dialog.ariaLabel = options.label;
  preview.classList.add('context-preview');
  dialog.append(preview);
  document.body.append(dialog);
  dialog.addEventListener('cancel', event => { event.preventDefault(); options.close(); });
  dialog.addEventListener('click', event => { if (event.target === dialog) options.close(); });
  options.mountMenu(dialog);
  dialog.showModal();
  dialog.toggleAttribute('data-scroll', dialog.scrollHeight > dialog.clientHeight);

  if (options.anchor?.isConnected) {
    const row = options.anchor.getBoundingClientRect(), card = preview.getBoundingClientRect(), box = dialog.getBoundingClientRect();
    const css = getComputedStyle(dialog);
    const top = (parseFloat(css.top) || 0) + 4;
    const bottom = (window.visualViewport?.height ?? innerHeight) - (parseFloat(css.bottom) || 0) - 4;
    const centered = row.top + (row.height - card.height) / 2 - (card.top - box.top);
    dialog.style.top = `${Math.max(top, Math.min(centered, bottom - box.height))}px`;
    dialog.style.bottom = 'auto';
    dialog.style.marginBlock = '0';
  }
  return dialog;
}
