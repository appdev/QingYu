import { popupMenu, type MenuItem } from './popup-menu';
import type { Notebook } from './notebook';

type FolderType = 'all' | 'folder' | 'root' | 'trash';
export function folderMenuItems(type: FolderType, clicked: boolean): MenuItem[] {
  const category = type === 'folder' || type === 'root';
  const items: MenuItem[] = [
    { text: '新建文件夹', shortcut: '⇧⌘N', enabled: false },
    { text: '新建子文件夹', enabled: category },
    { text: '重命名文件夹', shortcut: '⇧F6', enabled: type === 'folder' },
    { text: '移动到…', enabled: type === 'folder' },
    { text: '在 Finder 中显示', shortcut: '⌥⌘R', enabled: category || type === 'trash' },
    { text: '' },
    { text: '在终端中打开', shortcut: '⌘J', enabled: type !== 'trash' },
    { text: '移到废纸篓', shortcut: '⇧⌘⌫', enabled: type === 'folder' },
  ];
  // SidebarProjectView.willOpenMenu hides invalid items on rows, disables them on blank space.
  return clicked ? items.filter(item => item.enabled) : items;
}

export function createFolderMenu(getBook: () => Notebook, actions: {
  create(parentId?: string): Promise<void>;
  change(run: () => Promise<void>): Promise<void>;
  error(message: string): void;
}) {
  let finishEdit: (() => Promise<boolean>) | undefined;
  function rename(id: string) {
    const folder = getBook().data.folders.find(folder => folder.id === id);
    const row = document.querySelector<HTMLElement>(`.folder-row[data-folder-id="${CSS.escape(id)}"]`), label = row?.querySelector('.folder-name');
    if (!folder || folder.id === 'default' || folder.path === '' || !label || label.querySelector('input')) return;
    // A native WebKit button hides nested text fields from accessibility; edit in a group instead.
    const editor = document.createElement('div'); editor.className = row!.className + ' renaming'; editor.style.cssText = row!.style.cssText;
    editor.role = 'group'; editor.ariaLabel = '重命名文件夹'; editor.dataset.folderId = id;
    editor.append(...row!.childNodes); row!.replaceWith(editor);
    const input = document.createElement('input'); input.className = 'rename-folder'; input.ariaLabel = '文件夹名称'; input.value = folder.name; input.maxLength = 80;
    label.replaceChildren(input); input.focus(); input.select();
    let finishing = false;
    let completion: Promise<boolean> | undefined;
    const finish = (cancel = false): Promise<boolean> => {
      if (finishing) return completion ?? Promise.resolve(false); finishing = true;
      completion = (async () => {
        if (cancel || input.value.trim() === folder.name) { label.textContent = folder.name; row!.append(...editor.childNodes); editor.replaceWith(row!); row?.focus(); finishEdit = undefined; return true; }
        try { await actions.change(() => getBook().folderAction('rename', id, input.value)); finishEdit = undefined; return true; }
        catch (reason) { finishing = false; input.ariaInvalid = 'true'; actions.error(reason instanceof Error ? reason.message : String(reason)); input.focus(); return false; }
      })();
      return completion;
    };
    finishEdit = () => finish();
    input.onkeydown = event => {
      event.stopPropagation(); if (event.isComposing) return;
      if (event.key === 'Enter' || event.key === 'Escape') { event.preventDefault(); void finish(event.key === 'Escape'); }
    };
    input.onclick = event => event.stopPropagation();
    input.onblur = () => { void finish(); };
  }
  const open = async (id: string | null, type: FolderType, clicked: boolean, x: number, y: number) => {
    const items = folderMenuItems(type, clicked).filter(item => !getBook().server || !['在 Finder 中显示', '在终端中打开', '移到废纸篓'].includes(item.text));
    const run = (action: 'move' | 'trash' | 'reveal' | 'terminal') => actions.change(() => getBook().folderAction(action, type === 'trash' ? 'trash' : id));
    for (const item of items) {
      if (item.text === '移动到…' && getBook().server) {
        const folder = getBook().data.folders.find(folder => folder.id === id);
        item.items = getBook().data.folders.filter(other => other.id !== id && !other.path?.startsWith(folder?.path + '/'))
          .map(other => ({ text: other.path || other.name, run: () => actions.change(() => getBook().folderAction('move', id, other.id)) }));
        continue;
      }
      item.run = ({
      '新建文件夹': () => actions.create(), '新建子文件夹': () => actions.create(id ?? undefined),
      '重命名文件夹': () => { if (id) rename(id); }, '移动到…': () => run('move'),
      '在 Finder 中显示': () => run('reveal'), '在终端中打开': () => run('terminal'), '移到废纸篓': () => run('trash'),
    } as Record<string, () => void | Promise<void>>)[item.text];
    }
    await popupMenu(items, x, y, actions.error, () => document.querySelector<HTMLElement>('.folder-row[aria-pressed=true]')?.focus());
  };
  return { open, rename, finishRename: () => finishEdit?.() ?? Promise.resolve(true) };
}
