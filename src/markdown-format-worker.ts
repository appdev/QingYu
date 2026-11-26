import { formatMarkdown } from './typography-format';

// Prettier 加上 Markdown 插件约 380 KB，只在第一次排版时加载；长笔记格式化也不占用编辑器所在的线程。
self.onmessage = async ({ data }: MessageEvent<{ id: number; source: string }>) => {
  try {
    self.postMessage({ id: data.id, source: await formatMarkdown(data.source) });
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error) });
  }
};
