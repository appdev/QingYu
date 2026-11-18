import { popupMenu, type MenuItem } from './popup-menu';
import { noteTitle, visibleNotes, type Notebook } from './notebook';
import { noteText } from './document-text';

type FolderType = 'all' | 'folder' | 'root' | 'trash';
export function folderMenuItems(type: FolderType, clicked: boolean): MenuItem[] {
  const create = [{ text: '新建笔记' }, { text: '新建文件夹' }];
  const terminal = { text: '在终端中打开', shortcut: '⌘J' };
  const finder = { text: '在 Finder 中显示', shortcut: '⌥⌘R' };
  if (!clicked || type === 'all') return [...create, { text: '' }, terminal];
  if (type === 'trash') return [finder];
  return [
    { text: '在新窗口打开', enabled: false }, { text: '' }, ...create, { text: '' },
    { text: '重命名文件夹', shortcut: '⇧F6', enabled: type === 'folder' },
    { text: '制作副本', enabled: false },
    ...(type === 'folder' ? [{ text: '移动到…' }] : []),
    ...(type === 'folder' ? [{ text: '' }, { text: '上移' }, { text: '下移' }] : []),
    { text: '' }, finder, terminal,
    ...(type === 'folder' ? [{ text: '' }, { text: '移到废纸篓', shortcut: '⇧⌘⌫', destructive: true }] : []),
  ];
}

export function createFolderMenu(getBook: () => Notebook, actions: {
  create(parentId?: string): Promise<void>;
  createNote(folderId: string | null): Promise<void>;
  change(run: () => Promise<void>): Promise<void>;
  reorder(id: string, direction: -1 | 1, apply: boolean): boolean;
  error(message: string): void;
  mobile: boolean;
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
        // 提交时侧栏要按新名字重建，去掉「正在改名」的标记。
        editor.dataset.committing = '';
        try { await actions.change(() => getBook().folderAction('rename', id, input.value)); finishEdit = undefined; return true; }
        catch (reason) { delete editor.dataset.committing; finishing = false; input.ariaInvalid = 'true'; actions.error(reason instanceof Error ? reason.message : String(reason)); input.focus(); return false; }
      })();
      return completion;
    };
    finishEdit = () => finish();
    input.onkeydown = event => {
      event.stopPropagation(); if (event.isComposing) return;
      if (event.key === 'Enter' || event.key === 'Escape') { event.preventDefault(); void finish(event.key === 'Escape'); }
    };
    input.onclick = event => event.stopPropagation();
    // 文件监听核对笔记库时会把界面临时设为 inert，WebKit 因此让输入框失焦；
    // 这不是用户点走了，核对完焦点会还回来，不能当成改名完成。
    input.onblur = () => { if (!input.closest('[inert]')) void finish(); };
  }
  const open = async (id: string | null, type: FolderType, clicked: boolean, x: number, y: number, editing = false) => {
    const book = getBook();
    const items = folderMenuItems(type, clicked).filter(({ text }) => {
      if (editing) return ['', '重命名文件夹', '上移', '下移'].includes(text);
      return !((book.server || actions.mobile) && ['在 Finder 中显示', '在终端中打开'].includes(text)) &&
        !(book.server && text === '移到废纸篓') && !(actions.mobile && ['在新窗口打开', '上移', '下移'].includes(text));
    });
    const run = (action: 'move' | 'trash' | 'reveal' | 'terminal') => actions.change(() => getBook().folderAction(action, type === 'trash' ? 'trash' : id));
    for (const item of items) {
      if (id && (item.text === '上移' || item.text === '下移')) {
        const direction = item.text === '上移' ? -1 : 1;
        item.enabled = actions.reorder(id, direction, false);
        item.run = () => { actions.reorder(id, direction, true); };
        continue;
      }
      if (item.text === '移动到…' && book.server) {
        const folder = book.data.folders.find(folder => folder.id === id)!;
        const parent = folder.path?.includes('/') ? folder.path.slice(0, folder.path.lastIndexOf('/')) : '';
        item.items = book.data.folders.filter(other => other.id !== id && other.path !== parent && !other.path?.startsWith(folder.path + '/'))
          .map(other => ({ text: other.path || other.name, run: () => actions.change(() => getBook().folderAction('move', id, other.id)) }));
        item.enabled = item.items.length > 0;
        continue;
      }
      item.run = ({
        '新建笔记': () => actions.createNote(id), '新建文件夹': () => actions.create(id ?? undefined),
        '重命名文件夹': () => { if (id) rename(id); }, '移动到…': () => run('move'),
        '在 Finder 中显示': () => run('reveal'), '在终端中打开': () => run('terminal'), '移到废纸篓': () => run('trash'),
      } as Record<string, () => void | Promise<void>>)[item.text];
      if ((!book.root || actions.mobile) && ['在 Finder 中显示', '在终端中打开', '移动到…', '移到废纸篓'].includes(item.text)) item.enabled = false;
    }
    const menu = items.filter((item, index) => item.text || index > 0 && index < items.length - 1 && Boolean(items[index + 1].text));
    if (!menu.length) return;
    const preview = actions.mobile && clicked && !editing ? document.createElement('article') : undefined;
    if (preview) {
      const notes = visibleNotes(book.data, id, '', type === 'trash'), cards = document.createElement('div');
      cards.className = 'context-folder-preview';
      for (const note of notes.slice(0, 4)) {
        const card = document.createElement('div'), title = document.createElement('strong'), summary = document.createElement('p');
        title.textContent = noteTitle(note); summary.textContent = noteText(note.source).summary;
        card.append(title, summary); cards.append(card);
      }
      const title = document.createElement('strong'), count = document.createElement('p');
      title.textContent = book.data.folders.find(folder => folder.id === id)?.name ?? '全部笔记';
      count.textContent = `${notes.length} 篇笔记`;
      preview.append(cards, title, count);
    }
    const restoreFocus = () => [...document.querySelectorAll<HTMLElement>('.folder-row')].find(row =>
      (row.dataset.folderId || null) === id && (row.dataset.trash === 'true') === (type === 'trash'))?.focus({ preventScroll: true });
    await popupMenu(menu, x, y, actions.error, restoreFocus, '文件夹操作', preview);
  };
  return { open, rename, finishRename: () => finishEdit?.() ?? Promise.resolve(true) };
}
