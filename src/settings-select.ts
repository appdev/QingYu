export function settingsSelect(select: HTMLSelectElement, title: string | (() => string), mobile: boolean) {
  const element = document.createElement('span'); element.className = 'settings-select';
  element.append(select);
  element.insertAdjacentHTML('beforeend', '<span class="settings-select-icon" aria-hidden="true"><svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="m4.5 6 3.5-3.5L11.5 6m-7 4 3.5 3.5 3.5-3.5"/></svg></span>');
  if (!mobile) return { element, id: select.id, update() {}, destroy() {} };
  const button = document.createElement('button'); button.type = 'button'; button.className = 'settings-select-button'; button.id = `settings-select-${select.name}`;
  button.setAttribute('aria-haspopup', 'menu'); button.setAttribute('aria-expanded', 'false');
  if (select.getAttribute('aria-describedby')) button.setAttribute('aria-describedby', select.getAttribute('aria-describedby')!);
  select.hidden = true; element.append(button);
  const label = document.createElement('span'); button.append(label);
  let dismiss: (() => void) | undefined;
  const labelFor = () => typeof title === 'string' ? title : title();
  function update() {
    const text = select.selectedOptions[0]?.textContent ?? '';
    label.textContent = text; button.setAttribute('aria-label', `${labelFor()}：${text}`);
    button.disabled = select.disabled;
  }
  button.onclick = () => {
    if (dismiss) return;
    const dialog = document.createElement('dialog'); dialog.className = 'settings-select-popup'; dialog.setAttribute('aria-label', labelFor());
    const menu = document.createElement('div'); menu.role = 'menu'; menu.ariaLabel = labelFor(); dialog.append(menu);
    const buttons: HTMLButtonElement[] = [];
    function dispose() {
      window.removeEventListener('popstate', dispose); window.removeEventListener('resize', position);
      dialog.remove(); dismiss = undefined; button.setAttribute('aria-expanded', 'false'); button.focus({ preventScroll: true });
    }
    const close = () => history.back();
    for (const option of select.options) {
      const item = document.createElement('button'); item.type = 'button'; item.role = 'menuitemradio'; item.textContent = option.textContent;
      item.disabled = option.disabled; item.setAttribute('aria-checked', String(option.selected));
      item.onclick = () => { select.value = option.value; select.dispatchEvent(new Event('change', { bubbles: true })); update(); close(); };
      menu.append(item); buttons.push(item);
    }
    dialog.onkeydown = event => {
      const enabled = buttons.filter(item => !item.disabled), index = enabled.indexOf(document.activeElement as HTMLButtonElement);
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        enabled[event.key === 'Home' ? 0 : event.key === 'End' ? enabled.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + enabled.length) % enabled.length]?.focus();
      }
    };
    dialog.oncancel = event => { event.preventDefault(); close(); };
    dialog.onclick = event => {
      const box = dialog.getBoundingClientRect();
      if (event.target === dialog && (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom)) close();
    };
    function position() {
      const anchor = button.getBoundingClientRect();
      const right = button.closest('.settings-page')!.getBoundingClientRect().right;
      dialog.style.left = `${Math.max(12, Math.min(right - dialog.offsetWidth, innerWidth - dialog.offsetWidth - 12))}px`;
      dialog.style.top = `${Math.max(12, Math.min(anchor.bottom + 6, innerHeight - dialog.offsetHeight - 12))}px`;
    }
    history.pushState({ ...history.state, settings: true, settingsSelect: true }, '');
    button.setAttribute('aria-expanded', 'true'); dismiss = dispose;
    window.addEventListener('popstate', dispose, { once: true }); window.addEventListener('resize', position);
    element.closest('.settings')!.append(dialog); dialog.showModal(); position();
    (buttons.find(item => item.getAttribute('aria-checked') === 'true') ?? buttons[0])?.focus({ preventScroll: true });
  };
  update();
  return { element, id: button.id, update, destroy() { dismiss?.(); } };
}
