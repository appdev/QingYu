import { expect, it } from 'vitest';
import { diffVersionLines } from '../src/note-version-diff';

it('shows removed and added lines in place while ignoring line-ending differences', () => {
  expect(diffVersionLines('# 标题\r\n旧正文\r\n\r\n**保留**\r\n', '# 标题\n新正文\n\n**保留**\n')).toEqual([
    { text: '# 标题' },
    { text: '旧正文', change: 'removed' },
    { text: '新正文', change: 'added' },
    { text: '' },
    { text: '**保留**' },
    { text: '' },
  ]);
});

it('keeps both versions reconstructible when a large replacement exceeds the diff budget', () => {
  const before = ['# 标题', ...Array.from({ length: 1500 }, (_, i) => `旧行 ${i}`), '结尾'];
  const after = ['# 标题', ...Array.from({ length: 1500 }, (_, i) => `新行 ${i}`), '结尾'];
  const rows = diffVersionLines(before.join('\n'), after.join('\n'));
  expect(rows.filter(row => row.change !== 'added').map(row => row.text)).toEqual(before);
  expect(rows.filter(row => row.change !== 'removed').map(row => row.text)).toEqual(after);
  expect(rows.filter(row => !row.change)).toEqual([{ text: '# 标题' }, { text: '结尾' }]);
});
