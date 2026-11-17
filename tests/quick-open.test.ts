import { expect, it } from 'vitest';
import { quickOpenScore, quickOpenTextHits } from '../src/quick-open';
import type { Note } from '../src/notebook';

it('ranks exact, prefix, word-start and substring hits in that order', () => {
  expect(['预算', '预算清单', '每周 预算', '每周预算', '清单'].map(title => quickOpenScore(title, '预算')))
    .toEqual([0, 1, 2, 3, Infinity]);
});

it('finds visible Markdown body text without leaking front matter or inaccessible notes', async () => {
  const base = { folderId: 'default', createdAt: 0, updatedAt: 0 };
  const notes: Note[] = [
    { ...base, id: 'visible', source: '---\nmetadata: 预算\n---\r\n# 计划\r\n\r\n**预算**已确认\r\n本月预算\n下月预算\n全年预算' },
    { ...base, id: 'read-only', source: '预算', readOnly: true },
    { ...base, id: 'trashed', source: '预算', trashed: true },
  ];
  const controller = new AbortController();
  expect(await quickOpenTextHits(notes, '预算', controller.signal))
    .toEqual([{ note: notes[0], lines: ['预算已确认', '本月预算', '下月预算', '另有 1 处匹配'] }]);
  controller.abort();
  expect(await quickOpenTextHits(notes, '预算', controller.signal)).toEqual([]);
});
