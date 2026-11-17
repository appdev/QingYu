import { describe, expect, it } from 'vitest';
import { describeNoteLink, noteHeadingOffset, noteLinkChoices, noteLinkHeadingChoices, noteLinkMarkdown, noteLinkTargets, resolveNoteLink } from '../src/note-links';
import { type Library } from '../src/notebook';

const library: Library = {
  version: 1, activeId: 'a',
  folders: [{ id: 'personal', name: '个人' }, { id: 'projects', name: '项目' }],
  notes: [
    { id: 'a', title: '计划', folderId: 'personal', source: '', createdAt: 1, updatedAt: 1 },
    { id: 'b', title: '计划', folderId: 'projects', source: '', createdAt: 1, updatedAt: 2 },
    { id: 'c', title: '旧计划', folderId: 'projects', source: '', createdAt: 1, updatedAt: 3, trashed: true },
  ],
};

describe('Pinto note links', () => {
  it('uses the current folder for an unqualified title and resolves a qualified title and heading', () => {
    expect(resolveNoteLink(library, '计划', library.notes[0]).note?.id).toBe('a');
    expect(resolveNoteLink(library, '项目/计划#下周', library.notes[0])).toEqual({ note: library.notes[1], heading: '下周' });
    expect(resolveNoteLink(library, '旧计划', library.notes[0]).note).toBeUndefined();
  });
  it('matches actual headings by title or fragment without landing in code or frontmatter', async () => {
    const source = [
      '---\n# 九月 计划\n---',
      '```md\n# 九月 计划\n```',
      '九月 **计划**\n--------',
      '## 其他 [内容](https://example.com) ###',
      '## 附件 [[内部/草稿|草稿卡片]]',
      '## 项目说明',
      '## 项目说明',
    ].join('\n\n');
    expect(await noteHeadingOffset(source, '九月   计划')).toBe(source.indexOf('九月 **'));
    expect(await noteHeadingOffset(source, '九月-计划')).toBe(source.indexOf('九月 **'));
    expect(await noteHeadingOffset(source, '其他 **内容**')).toBeUndefined();
    expect(await noteHeadingOffset(source, '其他 内容')).toBe(source.indexOf('## 其他'));
    expect(await noteHeadingOffset(source, '附件 草稿卡片')).toBe(source.indexOf('## 附件'));
    expect(await noteHeadingOffset(source, '项目说明')).toBe(source.indexOf('## 项目说明'));
  });
  it('offers a qualified title for a duplicate and keeps the selected words as its alias', () => {
    expect(noteLinkChoices(library, 'a', '计划').map(note => note.target)).toEqual(['项目/计划']);
    expect(noteLinkMarkdown('项目/计划', '详细安排')).toBe('[[项目/计划|详细安排]]');
    expect(noteLinkMarkdown('项目/计划', '计划')).toBe('[[项目/计划]]');
  });

  it('suggests real headings from the resolved note, prioritizing prefixes and using the active draft', async () => {
    const source = [
      '---\n# 发布元数据\n---',
      '# 发布计划',
      '```md\n# 发布代码\n```',
      '## 历史发布',
      '## **发布安排**',
      '## 发布安排',
      '## 发布 [[旧计划|参考]]',
    ].join('\n\n');
    const other = { ...library.notes[1], source };
    const withHeadings = { ...library, notes: [library.notes[0], other, library.notes[2]] };
    expect(await noteLinkHeadingChoices(withHeadings, library.notes[0], '项目/计划#发布')).toEqual([
      { title: '发布安排', target: '项目/计划#发布安排' },
      { title: '发布 参考', target: '项目/计划#发布 参考' },
      { title: '历史发布', target: '项目/计划#历史发布' },
    ]);
    expect(await noteLinkHeadingChoices(withHeadings, library.notes[0], '计划#这', '# 计划\n\n## 这次讨论')).toEqual([
      { title: '这次讨论', target: '计划#这次讨论' },
    ]);
    expect(await noteLinkHeadingChoices(withHeadings, library.notes[0], '旧计划#')).toEqual([]);
    expect(await noteLinkHeadingChoices({ ...withHeadings, notes: [library.notes[0], { ...other, readOnly: true }] }, library.notes[0], '项目/计划#')).toEqual([]);
  });

  it('provides the summary and folder for note cards without treating code or regular links as cards', () => {
    const withSummary = { ...library, notes: library.notes.map(note => note.id === 'b' ? { ...note, source: '# 计划\n\n准备下周材料' } : note) };
    expect(describeNoteLink(withSummary, '项目/计划', library.notes[0])).toEqual({ detail: '准备下周材料', folder: '项目' });
    expect([...noteLinkTargets('[[旧计划]]\n\n![[项目/计划|卡片]]\n\n![[图片标题]](image.png)\n\n```md\n![[代码示例]]\n```', true)]).toEqual(['项目/计划']);
  });
});
