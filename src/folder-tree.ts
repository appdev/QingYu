import type { Folder } from './notebook';

export type FolderRow = { folder: Folder; depth: number; parentId: string | null; hasChildren: boolean };

// Sidebar.getList hides the library root; subfolders(of:) sorts each level separately.
export function folderRows(folders: Folder[], expanded: ReadonlySet<string>, order: readonly string[] = []): FolderRow[] {
  const visible = folders.filter(folder => folder.id !== 'default' && folder.path !== '');
  const paths = new Map(visible.map(folder => [folder.path ?? folder.name, folder.id]));
  const children = new Map<string | null, Folder[]>();
  for (const folder of visible) {
    const path = folder.path ?? folder.name, parent = paths.get(path.slice(0, path.lastIndexOf('/') < 0 ? 0 : path.lastIndexOf('/'))) ?? null;
    const siblings = children.get(parent) ?? []; siblings.push(folder); children.set(parent, siblings);
  }
  const rows: FolderRow[] = [], collator = new Intl.Collator(undefined, { sensitivity: 'base' });
  const rank = new Map(order.map((id, index) => [id, index]));
  function visit(parentId: string | null, depth: number) {
    for (const folder of (children.get(parentId) ?? []).sort((a, b) =>
      (rank.get(a.id) ?? order.length) - (rank.get(b.id) ?? order.length) || collator.compare(a.name, b.name))) {
      rows.push({ folder, depth, parentId, hasChildren: Boolean(children.get(folder.id)?.length) });
      if (expanded.has(folder.id)) visit(folder.id, depth + 1);
    }
  }
  visit(null, 0); return rows;
}
