interface Window {
  nativeAppearance?: { postMessage(theme: string): void };
  MiaoYanHeadingLinks: {
    find(root: ParentNode, href: string): HTMLElement | null;
    follow(root: ParentNode, href: string): boolean;
  };
}

declare module 'markdown-it-task-lists' {
  import type { PluginSimple } from 'markdown-it';
  const plugin: PluginSimple;
  export default plugin;
}
declare module 'markdown-it-footnote' {
  import type { PluginSimple } from 'markdown-it';
  const plugin: PluginSimple;
  export default plugin;
}
declare module 'markdown-it/lib/rules_block/reference.mjs' {
  import type { RuleBlock } from 'markdown-it/lib/parser_block.mjs';
  const reference: RuleBlock;
  export default reference;
}
