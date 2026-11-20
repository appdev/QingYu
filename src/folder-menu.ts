import { formatLabel, translatedLabel } from './ui-language';
import { showMenu, type MenuItem } from './popup-menu';
import { noteTitle, visibleNotes, type Notebook } from './notebook';
import { noteText } from './document-text';
import { CopyPlus, Folder, SquarePen, Trash2 } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { folderRows } from './folder-tree';
import { showMoveFolderSheet } from './note-menu';
import { alertActions, showAlert } from './alert-dialog';
import folderPlusIcon from '../public/miaoyan/icons/folder-badge-plus.svg?raw';

type FolderType = 'all' | 'folder' | 'root' | 'trash';
const folderMenuIcons: Record<string, typeof Folder> = {
  新建笔记: SquarePen, 复制文件夹: CopyPlus, '移动到…': Folder, 移到废纸篓: Trash2,
};
const pencilIcon = '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M4 17.3 15.2 6.1l2.7 2.7L6.7 20H4zm12.5-12.5.9-.9a1.9 1.9 0 0 1 2.7 2.7l-.9.9z"/></svg>';

export function folderMenuItems(type: FolderType, clicked: boolean): MenuItem[] {
  const create = [{ text: '新建笔记' }, { text: '新建文件夹' }];
  const terminal = { text: '在终端中打开', shortcut: '⌘J' };
  const finder = { text: '在 Finder 中显示', shortcut: '⌥⌘R' };
  if (!clicked || type === 'all') return [...create, { text: '' }, terminal];
  if (type === 'trash') return [finder];
  return [
    { text: '在新窗口打开', enabled: false }, { text: '' }, ...create, { text: '' },
    { text: '重命名文件夹', shortcut: '⇧F6', enabled: type === 'folder' },
    { text: '复制文件夹', enabled: false },
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
    editor.role = 'group'; editor.ariaLabel = translatedLabel('app.renameMarkdownFolder'); editor.dataset.folderId = id;
    editor.append(...row!.childNodes); row!.replaceWith(editor);
    const input = document.createElement('input'); input.className = 'rename-folder'; input.ariaLabel = translatedLabel('文件夹名称'); input.value = folder.name; input.maxLength = 80;
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
    // Pinto 的手机端在「全部笔记」「废纸篓」上长按，只有一个「新建文件夹」，预览就是这一行本身。
    const libraryRow = actions.mobile && clicked && !editing && (type === 'all' || type === 'trash');
    const items = (libraryRow ? [{ text: '新建文件夹' }] as MenuItem[] : folderMenuItems(type, clicked)).filter(({ text }) => {
      if (editing) return ['', '重命名文件夹', '上移', '下移'].includes(text);
      // Finder、终端和新窗口依赖本机系统；复制和删除文件夹网页客户端由部署端的笔记库执行
      if (!book.root && ['在 Finder 中显示', '在终端中打开', '在新窗口打开'].includes(text)) return false;
      if (!book.root && !book.server && ['复制文件夹', '移到废纸篓'].includes(text)) return false;
      if (!book.root && !book.server && text === '移动到…') return false;
      if (text === '复制文件夹' && type !== 'folder' && actions.mobile) return false;
      return !(actions.mobile && ['在 Finder 中显示', '在终端中打开', '在新窗口打开', '上移', '下移'].includes(text));
    });
    const findRow = () => [...document.querySelectorAll<HTMLElement>('.folder-row')].find(row =>
      (row.dataset.folderId || null) === id && (row.dataset.trash === 'true') === (type === 'trash'));
    const restoreFocus = () => findRow()?.focus({ preventScroll: true });
    const run = (action: Parameters<Notebook['folderAction']>[0], value?: string) =>
      actions.change(() => getBook().folderAction(action, type === 'trash' ? 'trash' : id, value));
    const report = (reason: unknown) => actions.error(reason instanceof Error ? reason.message : String(reason));
    // 移到笔记库自己的废纸篓，之后在废纸篓里恢复；App 移动端和网页客户端共用
    const confirmLocalTrash = (name: string) => {
      const dialog = document.createElement('dialog'), title = document.createElement('h2'), message = document.createElement('p');
      title.id = 'folder-trash-title'; title.textContent = translatedLabel('移到废纸篓？');
      message.textContent = formatLabel('「{name}」和里面的笔记会移到废纸篓。', { name });
      dialog.setAttribute('aria-labelledby', title.id);
      const confirm = document.createElement('button'), cancel = document.createElement('button');
      cancel.textContent = translatedLabel('取消'); confirm.textContent = translatedLabel('moveToTrash'); confirm.className = 'destructive-button';
      cancel.onclick = () => dialog.close();
      confirm.onclick = () => { dialog.close(); void run('trash-local').catch(report); };
      dialog.addEventListener('close', () => { dialog.remove(); restoreFocus(); }, { once: true });
      dialog.append(title, message, alertActions(confirm, cancel));
      showAlert(dialog, cancel);
    };
    for (const item of items) {
      const icon = folderMenuIcons[item.text];
      if (actions.mobile && icon) item.icon = createLucideIcon(document, icon, '');
      if (actions.mobile && (item.text === '新建文件夹' || item.text === '重命名文件夹')) {
        const template = document.createElement('template');
        template.innerHTML = item.text === '新建文件夹' ? folderPlusIcon : pencilIcon;
        item.icon = template.content.firstElementChild as SVGElement;
      }
      if (id && (item.text === '上移' || item.text === '下移')) {
        const direction = item.text === '上移' ? -1 : 1;
        item.enabled = actions.reorder(id, direction, false);
        item.run = () => { actions.reorder(id, direction, true); };
        continue;
      }
      if (item.text === '移动到…' && book.server && !actions.mobile) {
        const folder = book.data.folders.find(folder => folder.id === id)!;
        const parent = folder.path?.includes('/') ? folder.path.slice(0, folder.path.lastIndexOf('/')) : '';
        item.items = book.data.folders.filter(other => other.id !== id && other.path !== parent && !other.path?.startsWith(folder.path + '/'))
          .map(other => ({ text: other.path || other.name, run: () => actions.change(() => getBook().folderAction('move', id, other.id)) }));
        item.enabled = item.items.length > 0;
        continue;
      }
      if (actions.mobile && (book.root || book.server) && type === 'folder') {
        const folder = book.data.folders.find(folder => folder.id === id)!;
        if (item.text === '复制文件夹') { item.enabled = true; item.run = () => run('duplicate'); continue; }
        if (item.text === '移动到…') {
          const path = folder.path ?? folder.name;
          const parent = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
          const rows = [{ folder: { id: 'default', name: '笔记库顶层', path: '' }, depth: 0 },
            ...folderRows(book.data.folders, new Set(book.data.folders.map(folder => folder.id)))].map(({ folder, depth }) => ({
            name: folder.name, depth, current: folder.path === parent,
            disabled: folder.id === id || folder.path?.startsWith(path + '/'),
            run: () => { void run('move-to-folder', folder.id).catch(report); },
          }));
          item.enabled = rows.some(row => !row.current && !row.disabled);
          item.run = () => showMoveFolderSheet(rows, restoreFocus);
          continue;
        }
        if (item.text === '移到废纸篓') { item.run = () => confirmLocalTrash(folder.name); continue; }
      }
      // 桌面 App 移到系统废纸篓；网页客户端没有系统废纸篓，和移动端一样移到笔记库废纸篓
      if (book.server && !actions.mobile && type === 'folder' && id && item.text === '移到废纸篓') {
        const folder = book.data.folders.find(folder => folder.id === id)!;
        item.run = () => confirmLocalTrash(folder.name);
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
    if (preview) preview.dataset.folderPreview = '';
    const row = findRow();
    if (preview && libraryRow) {
      preview.dataset.row = '';
      const icon = row?.querySelector('.sidebar-icon')?.cloneNode(true), name = document.createElement('strong'), count = document.createElement('span');
      name.textContent = row?.querySelector('.folder-name')?.textContent ?? (type === 'trash' ? '废纸篓' : '全部笔记');
      count.textContent = row?.querySelector('.folder-count')?.textContent ?? '';
      if (icon) preview.append(icon);
      preview.append(name, count);
    } else if (preview) {
      const notes = visibleNotes(book.data, id, '', type === 'trash'), cards = document.createElement('div');
      cards.className = 'context-folder-preview';
      for (const note of notes.slice(0, 4)) {
        const card = document.createElement('div'), title = document.createElement('strong'), summary = document.createElement('p');
        title.textContent = noteTitle(note); summary.textContent = noteText(note.source).summary;
        card.append(title, summary); cards.append(card);
      }
      const title = document.createElement('strong'), count = document.createElement('p');
      title.textContent = book.data.folders.find(folder => folder.id === id)?.name ?? '全部笔记';
      count.textContent = formatLabel('notesCount', { count: notes.length });
      if (cards.childElementCount) preview.append(cards);
      preview.append(title, count);
    }
    await showMenu(menu, { label: '文件夹操作', at: { x, y }, preview, previewAnchor: preview ? row : undefined, onError: actions.error, restoreFocus });
  };
  return { open, rename, finishRename: () => finishEdit?.() ?? Promise.resolve(true) };
}
