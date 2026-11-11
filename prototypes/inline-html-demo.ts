import Vditor from 'vditor';
import 'vditor/dist/index.css';
import '../src/rich-editor.css';
import { installRichMarkdown } from '../src/rich-markdown';

const source = '# HTML 保真原型\n\n'
  + 'M1 前<mark>高亮内容</mark>后\n\n'
  + 'M2 水 H<sub>2</sub>O，x<sup>2</sup>。\n\n'
  + 'M3 前<u>下划线内容</u>后，按 <kbd>Ctrl</kbd>。\n\n'
  + 'M4 <MARK>嵌套 <u>下划线</u> 和 **Markdown 粗体**</MARK>。\n\n'
  + '- M5 <mark>列表<br>换行</mark>\n\n'
  + '> M6 <kbd>引用</kbd>\n\n'
  + '| 列 |\n|---|\n| M7 <u>表格</u> |\n\n'
  + 'M8 <mark>跨节点</mark>普通<u>尾部</u>\n\n'
  + 'M9 字面 ==text== ~text~ ^text^ `<mark>代码</mark>`\n\n'
  + '[引用][a]\n\n[a]: <https://example.com> \'标题 title\'\n';
const output = document.querySelector<HTMLTextAreaElement>('#writeback')!;
let editor: Vditor;
await new Promise<void>(resolve => {
  editor = new Vditor('rich-editor', {
    cdn: '/vditor', lang: 'zh_CN', mode: 'wysiwyg', height: 560,
    cache: { enable: false }, toolbar: ['bold', 'italic', 'undo', 'redo'],
    preview: { markdown: { sanitize: true, gfmAutoLink: false } },
    input: value => { output.value = value; }, after: resolve,
  });
});
const instance = editor!;
installRichMarkdown(instance.vditor.lute);
instance.setTheme('dark');
function reset(value = source) {
  instance.setValue(value, true);
  output.value = instance.getValue();
}
document.querySelector('#reset')!.addEventListener('click', () => reset());
reset();
Object.assign(window, { htmlPrototype: { editor: instance, source, reset, read: () => instance.getValue() } });
