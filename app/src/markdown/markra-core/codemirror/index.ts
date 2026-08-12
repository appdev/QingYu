import { markdown } from "@codemirror/lang-markdown";
import { codeFolding } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { GFM } from "@lezer/markdown";
import {
  markdownSyntaxHighlighting,
  markraHighlight,
} from "./highlight";
import { imageAttributesMarkdown } from "./image-attributes";
import {
  markraPlugins,
  type MarkraPlugin,
} from "./plugin";
import { livePreview, type LivePreviewConfig } from "./preview";
import { markraSlashMenu } from "./slash-menu";
import { markraTheme } from "./theme";

export type {
  BlockCommandId,
  BlockLabels,
  BlocksPluginOptions,
} from "./blocks";
export { blocksPlugin } from "./blocks";
export type {
  CodeMirrorBlockDragLabels,
  CodeMirrorBlockDragPluginOptions,
  CodeMirrorBlockDropSide,
  CodeMirrorBlockRange,
} from "./block-drag";
export {
  addCodeMirrorBlockBelow,
  codeMirrorBlockDragPlugin,
  moveCodeMirrorBlock,
  readCodeMirrorBlockRanges,
} from "./block-drag";
export type { CodeMirrorClipboardAssetsPluginOptions } from "./clipboard-assets";
export { codeMirrorClipboardAssetsPlugin } from "./clipboard-assets";
export type { CalloutPreviewPluginOptions } from "./callout-preview";
export { calloutPreviewPlugin } from "./callout-preview";
export type {
  CodeBlockHighlightContext,
  CodeBlockHighlightSpan,
  CodeBlockPreviewPluginOptions,
} from "./code-block";
export { codeBlockPreviewPlugin } from "./code-block";
export type {
  CodeMirrorDocumentAnchor,
  CodeMirrorHeadingAnchor,
  CodeMirrorMarkdownImageInsertionTarget,
  CodeMirrorSearchOptions,
  CodeMirrorMarkdownImageReference,
  CodeMirrorMarkdownLinkReference,
  ReplaceCodeMirrorMarkdownOptions,
} from "./controller";
export {
  captureCodeMirrorMarkdownImageInsertionTarget,
  comparableCodeMirrorMarkdown,
  codeMirrorSelectionIsInsideFencedCode,
  discardCodeMirrorMarkdownImageInsertionTarget,
  findCodeMirrorSearchMatches,
  insertCodeMirrorMarkdownImage,
  insertCodeMirrorMarkdownImages,
  insertCodeMirrorMarkdownImagesAtTarget,
  insertCodeMirrorMarkdownLink,
  insertCodeMirrorMarkdownLinks,
  insertCodeMirrorMarkdownSnippet,
  insertCodeMirrorMarkdownTable,
  isCodeMirrorMarkdownEquivalent,
  isCodeMirrorMarkdownImageInsertionTargetActive,
  readCodeMirrorTextSelection,
  readCodeMirrorHeadingAnchors,
  readCodeMirrorSectionAnchors,
  readCodeMirrorTableAnchors,
  replaceAllCodeMirrorSearchMatches,
  replaceCodeMirrorMarkdown,
  replaceCodeMirrorSearchMatch,
  serializeCodeMirrorMarkdownImage,
  serializeCodeMirrorMarkdownLink,
  updateCodeMirrorHeadingAnchors,
} from "./controller";
export type {
  DocumentLinksPluginOptions,
  MarkraDocumentLinkAction,
  MarkraDocumentLinkItem,
  MarkraDocumentLinksContext,
  MarkraDocumentLinksState,
} from "./document-links";
export {
  closeMarkraDocumentLinks,
  documentLinksPlugin,
  getMarkraDocumentLinksState,
  runMarkraDocumentLink,
} from "./document-links";
export type {
  FormattingCommandId,
  FormattingLabels,
  FormattingPluginOptions,
} from "./formatting";
export {
  clearCodeMirrorSelectionFormatting,
  formattingPlugin,
} from "./formatting";
export { footnotePreviewPlugin } from "./footnote-preview";
export type { FoldToggleLabels, FoldTogglePluginOptions } from "./fold-toggle";
export { foldTogglePlugin } from "./fold-toggle";
export { toggleAllCodeMirrorFolds } from "./folding";
export { frontmatterHiddenPlugin } from "./frontmatter-preview";
export type {
  ImagePreviewPluginOptions,
  MarkraImageSourceContext,
} from "./image";
export { imagePreviewPlugin, resolveSafeImageSource } from "./image";
export type {ImageAtomicRange} from "./image-atomic";
export {
  clearImageAtomicSelection,
  getSelectedImageAtomicRange,
  imageAtomicEditingPlugin,
  readImageAtomicRanges,
  selectImageAtomicRange,
} from "./image-atomic";
export type { ImageAttributeDetails } from "./image-attributes";
export {
  imageAttributeListLength,
  imageAttributeDetails,
  imageAttributesMarkdown,
  replaceImageWidth,
} from "./image-attributes";
export {
  jsonSyntaxHighlighting,
  markdownSourceSyntaxHighlighting,
  markdownSyntaxHighlighting,
  markraHighlight,
} from "./highlight";
export { horizontalRulePlugin } from "./horizontal-rule";
export type {
  InsertionCommandId,
  InsertionLabels,
  InsertionsPluginOptions,
} from "./insertions";
export { insertionsPlugin } from "./insertions";
export { markdownEditingPlugin } from "./markdown-editing";
export type { MarkdownShortcutsPluginOptions } from "./markdown-shortcuts";
export { markdownShortcutsPlugin } from "./markdown-shortcuts";
export type {
  LinksPluginOptions,
  MarkraLinkActivation,
  MarkraLinkOpenContext,
  MarkraLinkSourceContext,
} from "./links";
export {
  linksPlugin,
  resolveAutolinkTarget,
  resolveSafeLinkTarget,
} from "./links";
export type { CodeMirrorMathRange } from "./math-preview";
export { findCodeMirrorMathRanges, mathPreviewPlugin } from "./math-preview";
export type { RevealContext, RevealPolicy, RevealScope } from "./policy";
export type {
  MarkraCommand,
  MarkraCommandContext,
  MarkraKeyBinding,
  MarkraPlugin,
  MarkraUiAction,
  MarkraUiContribution,
  MarkraUiPlacement,
} from "./plugin";
export type { LivePreviewConfig } from "./preview";
export type {
  MarkraRenderer,
  MarkraRendererContext,
  MarkraRendererScope,
  MarkraSyntaxNode,
} from "./renderers";
export { revealActiveLine } from "./policy";
export type { RawHtmlPreviewPluginOptions } from "./raw-html-preview";
export { rawHtmlPreviewPlugin } from "./raw-html-preview";
export {
  defineMarkraPlugin,
  listMarkraPlugins,
  listMarkraUi,
  markraPlugins,
  runMarkraCommand,
  searchMarkraUi,
} from "./plugin";
export type {
  MarkraSlashMenuSource,
  MarkraSlashMenuState,
} from "./slash-menu";
export {
  closeMarkraSlashMenu,
  getMarkraSlashMenuState,
  markraSlashMenu,
  openMarkraSlashMenu,
  runMarkraSlashMenuAction,
} from "./slash-menu";
export { livePreview } from "./preview";
export { markraRenderer } from "./renderers";
export type { CodeMirrorSearchState } from "./search";
export {
  codeMirrorSearchPlugin,
  getCodeMirrorSearchState,
  scrollCodeMirrorSearchMatchIntoView,
  updateCodeMirrorSearchDecorations,
} from "./search";
export {
  clearCodeMirrorSelectionHold,
  codeMirrorSelectionHoldPlugin,
  showCodeMirrorSelectionHold,
} from "./selection-hold";
export { markraTheme } from "./theme";
export { convertCodeMirrorClipboardHtml } from "./html-paste";
export type { TableFragmentMergePluginOptions } from "./table-fragment-merge";
export { tableFragmentMergePlugin } from "./table-fragment-merge";
export type {
  CodeMirrorTableAlignment,
  CodeMirrorTableShape,
  CodeMirrorTableWidthMode,
  TablePreviewPluginOptions,
} from "./table";
export { readCodeMirrorTableShape, tablePreviewPlugin } from "./table";
export { trailingSpacePlugin } from "./trailing-space";
export type { CodeMirrorTypewriterModeOptions } from "./typewriter";
export { codeMirrorTypewriterMode } from "./typewriter";
export type { CodeMirrorVimLabels } from "./vim";
export { reconfigureCodeMirrorVimMode } from "./vim";

export interface LiveMarkdownConfig extends LivePreviewConfig {
  highlight?: boolean;
  plugins?: readonly MarkraPlugin[];
  slashMenu?: boolean;
}

export const markraLanguage = markdown({
  extensions: [GFM, imageAttributesMarkdown, markraHighlight],
});

export function liveMarkdown(config: LiveMarkdownConfig = {}): Extension {
  const {
    highlight = true,
    plugins = [],
    slashMenu = false,
    ...previewConfig
  } = config;
  return [
    highlight
      ? markraLanguage
      : markdown({ extensions: [GFM, imageAttributesMarkdown] }),
    markdownSyntaxHighlighting,
    codeFolding(),
    livePreview(previewConfig),
    markraTheme,
    markraPlugins(plugins),
    slashMenu ? markraSlashMenu() : [],
  ];
}
