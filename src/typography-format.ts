import { format } from 'prettier/standalone';
import * as markdown from 'prettier/plugins/markdown';

/** 文档排版：和妙言一样用 Prettier 的 Markdown 规则，不重排段落换行，代码块里的代码不动。 */
export async function formatMarkdown(source: string) {
  const formatted = await format(source, {
    parser: 'markdown', plugins: [markdown], proseWrap: 'preserve', embeddedLanguageFormatting: 'off',
  });
  // Prettier 总会补一个结尾换行；原文没有就不加，免得只为这一个字符算作改动。
  return !source.endsWith('\n') && formatted.endsWith('\n') ? formatted.slice(0, -1) : formatted;
}
