import { useCallback, useEffect, useMemo, useRef } from "react";
import {
  defaultValueCtx,
  Editor,
  editorViewCtx,
  editorViewOptionsCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
  serializerCtx
} from "@milkdown/kit/core";
import { history } from "@milkdown/kit/plugin/history";
import { listener, listenerCtx } from "@milkdown/kit/plugin/listener";
import { Plugin } from "@milkdown/kit/prose/state";
import { imageSchema, linkSchema } from "@milkdown/kit/preset/commonmark";
import { Milkdown, MilkdownProvider, useEditor, useInstance } from "@milkdown/react";
import { $prose } from "@milkdown/kit/utils";
import {
  markraSelectionHoldPlugin,
  markraBlockDragPlugin,
  markraBlockGapPlugin,
  markraCalloutPlugin,
  markraCalloutRemarkPlugin,
  markraCalloutSerializerPlugin,
  markraClipboardImagePluginWithOptions,
  markraCodeBlockPlugin,
  markraVisualCaretPlugin,
  markraFrontmatterRemarkPlugin,
  markraFrontmatterSchema,
  markraFootnoteDefinitionInputPlugin,
  markraFootnotePreviewPlugin,
  markraFootnoteReferenceInputRule,
  markraHeadingLevelPlugin,
  markraHeadingTogglePlugin,
  markraListTogglePlugin,
  markraLinkImageLivePlugin,
  markraLiveMarkdownPlugin,
  markraLiveMarkdownSpecs,
  markraMarkdownSourcePastePlugin,
  markraMarkdownShortcuts,
  markraMathCaretAnchorSuppressionPlugin,
  markraMathPlugin,
  markraMathRemarkPlugin,
  markraMathSourcePlugin,
  markraRawHtmlPlugin,
  markraSearchPlugin,
  markraSlashCommands,
  markraTableControlsPlugin,
  markraTableFragmentMergePlugin,
  markraTaskListPlugin,
  markraTrailingParagraphPlugin,
  markraTaskListSchema,
  normalizeMarkdownShortcuts,
  restoreEscapedMarkdownSource,
  serializeLinkImageLiveMarkdown,
  type EditorTextSelection,
  type MarkdownShortcutMap,
  type SaveClipboardAttachment,
  type SaveClipboardImage,
  type SaveEditorResources,
  type SaveRemoteClipboardImage,
  type SlashCommandLabels
} from "@markra/editor";
import { t, type AppLanguage } from "@markra/shared";
import type { ExtendedSyntaxPreferences, TableColumnWidthModePreference } from "../lib/settings/app-settings";
import type { MarkdownDocumentLinkFile } from "../lib/document-links";
import { createDeferredMarkdownChangeEmitter } from "../lib/deferred-markdown-change";
import { markraDocumentLinkCompletionPlugin } from "./document-link-completion";
import {
  markraCommonmark,
  markraExternalLinkClickPlugin,
  markraGfm,
  readInlineHtmlMarkdownText,
  markraTextSelectionObserverPlugin,
  serializeMarkdownClipboardText
} from "./markdown-paper-plugins";

export type MarkdownPaperSurfaceProps = {
  autoFocus: boolean;
  documentPath?: string | null;
  initialContent: string;
  language: AppLanguage;
  extendedSyntax?: ExtendedSyntaxPreferences;
  markdownShortcuts?: MarkdownShortcutMap;
  onEditorReady: (editor: Editor | null, options?: { autoFocus?: boolean }) => unknown;
  onActiveOutlineIndexChange?: (index: number | null) => unknown;
  onMarkdownChange: (content: string) => unknown;
  onSaveClipboardAttachment?: SaveClipboardAttachment;
  onSaveClipboardImage?: SaveClipboardImage;
  onSaveEditorResources?: SaveEditorResources;
  onSaveRemoteClipboardImage?: SaveRemoteClipboardImage;
  openLocalAttachment?: (src: string) => unknown;
  openExternalUrl?: (url: string) => unknown;
  readOnly?: boolean;
  onTextSelectionChange?: (selection: EditorTextSelection | null) => unknown;
  resolveImageSrc?: (src: string) => string;
  tableColumnWidthMode?: TableColumnWidthModePreference;
  workspaceFiles?: MarkdownDocumentLinkFile[];
};

function markdownShortcutSignature(shortcuts: MarkdownShortcutMap | undefined) {
  return JSON.stringify(normalizeMarkdownShortcuts(shortcuts));
}

function MilkdownInstanceBridge({ autoFocus, onEditorReady }: Pick<MarkdownPaperSurfaceProps, "autoFocus" | "onEditorReady">) {
  const [loading, getEditor] = useInstance();
  const autoFocusRef = useRef(autoFocus);
  const onEditorReadyRef = useRef(onEditorReady);

  useEffect(() => {
    autoFocusRef.current = autoFocus;
  }, [autoFocus]);

  useEffect(() => {
    onEditorReadyRef.current = onEditorReady;
  }, [onEditorReady]);

  useEffect(() => {
    if (loading) return;

    const editor = getEditor();
    onEditorReadyRef.current(editor, { autoFocus: autoFocusRef.current });

    return () => {
      onEditorReadyRef.current(null);
    };
  }, [getEditor, loading]);

  return null;
}

function MilkdownReadOnlyBridge({ readOnly = false }: Pick<MarkdownPaperSurfaceProps, "readOnly">) {
  const [loading, getEditor] = useInstance();

  useEffect(() => {
    if (loading) return;

    const editor = getEditor();
    editor?.action((ctx) => {
      const view = ctx.get(editorViewCtx);
      view.setProps({
        editable: () => !readOnly
      });
      view.dom.setAttribute("aria-readonly", readOnly ? "true" : "false");
    });
  }, [getEditor, loading, readOnly]);

  return null;
}

function markraReadOnlyTransactionGuard(readOnlyRef: { current: boolean }) {
  return $prose(() => new Plugin({
    filterTransaction(transaction) {
      return !readOnlyRef.current || !transaction.docChanged;
    }
  }));
}

function MilkdownEditorSurface({
  autoFocus,
  documentPath,
  extendedSyntax,
  initialContent,
  language,
  markdownShortcuts,
  onEditorReady,
  onActiveOutlineIndexChange,
  onMarkdownChange,
  onSaveClipboardAttachment,
  onSaveClipboardImage,
  onSaveEditorResources,
  onSaveRemoteClipboardImage,
  openLocalAttachment,
  openExternalUrl,
  readOnly = false,
  onTextSelectionChange,
  resolveImageSrc,
  tableColumnWidthMode,
  workspaceFiles
}: MarkdownPaperSurfaceProps) {
  const initialContentRef = useRef(initialContent);
  const documentPathRef = useRef(documentPath);
  const onMarkdownChangeRef = useRef(onMarkdownChange);
  const onActiveOutlineIndexChangeRef = useRef(onActiveOutlineIndexChange);
  const openExternalUrlRef = useRef(openExternalUrl);
  const openLocalAttachmentRef = useRef(openLocalAttachment);
  const onSaveClipboardAttachmentRef = useRef(onSaveClipboardAttachment);
  const onSaveClipboardImageRef = useRef(onSaveClipboardImage);
  const onSaveEditorResourcesRef = useRef(onSaveEditorResources);
  const onSaveRemoteClipboardImageRef = useRef(onSaveRemoteClipboardImage);
  const onTextSelectionChangeRef = useRef(onTextSelectionChange);
  const readOnlyRef = useRef(readOnly);
  const resolveImageSrcRef = useRef(resolveImageSrc);
  const tableColumnWidthModeRef = useRef(tableColumnWidthMode ?? "auto");
  const workspaceFilesRef = useRef(workspaceFiles ?? []);
  const deferredMarkdownChangeRef = useRef<ReturnType<typeof createDeferredMarkdownChangeEmitter<() => unknown>> | null>(null);
  const externalLinkOpeningEnabled = Boolean(openExternalUrl || openLocalAttachment);
  const markdownDocumentLabel = t(language, "app.markdownDocument");
  const githubAlertsEnabled = extendedSyntax?.githubAlerts ?? true;
  const highlightSyntaxEnabled = extendedSyntax?.highlight ?? true;
  const shortcutsSignature = markdownShortcutSignature(markdownShortcuts);
  const normalizedMarkdownShortcuts = useMemo(
    () => normalizeMarkdownShortcuts(markdownShortcuts),
    [shortcutsSignature]
  );
  if (deferredMarkdownChangeRef.current === null) {
    deferredMarkdownChangeRef.current = createDeferredMarkdownChangeEmitter((emitMarkdownChange) => {
      emitMarkdownChange();
    });
  }
  const tableControlLabels = {
    addColumnRight: t(language, "editor.table.addColumnRight"),
    addRowBelow: t(language, "editor.table.addRowBelow"),
    alignLeft: t(language, "editor.table.alignLeft"),
    alignCenter: t(language, "editor.table.alignCenter"),
    alignRight: t(language, "editor.table.alignRight"),
    autoWidth: t(language, "editor.table.columnWidthMode"),
    deleteColumn: t(language, "editor.table.deleteColumn"),
    deleteRow: t(language, "editor.table.deleteRow"),
    deleteTable: t(language, "editor.table.deleteTable"),
    adjustTable: t(language, "editor.table.adjustTable"),
    resizeTableTo: t(language, "editor.table.resizeTableTo"),
    tableColumns: t(language, "editor.table.columns"),
    tableRows: t(language, "editor.table.rows")
  };
  const tableFragmentMergeLabel = t(language, "editor.table.mergeFragment");
  const blockDragLabels = {
    addBlock: t(language, "editor.blockAdd"),
    dragBlock: t(language, "editor.blockDrag")
  };
  const headingToggleLabels = {
    collapseSection: t(language, "editor.collapseSection"),
    expandSection: t(language, "editor.expandSection")
  };
  const headingLevelLabels = {
    paragraph: t(language, "menu.paragraph")
  };
  const listToggleLabels = {
    collapseListItem: t(language, "editor.collapseListItem"),
    expandListItem: t(language, "editor.expandListItem")
  };
  const slashCommandLabels = useMemo<SlashCommandLabels>(() => ({
    menu: t(language, "editor.slashCommands"),
    noResults: t(language, "editor.slashCommandsNoResults"),
    commands: {
      bulletList: t(language, "menu.bulletList"),
      callout: t(language, "menu.callout"),
      codeBlock: t(language, "menu.codeBlock"),
      heading1: t(language, "menu.heading1"),
      heading2: t(language, "menu.heading2"),
      heading3: t(language, "menu.heading3"),
      orderedList: t(language, "menu.orderedList"),
      paragraph: t(language, "menu.paragraph"),
      quote: t(language, "menu.quote"),
      table: t(language, "menu.table")
    }
  }), [language]);

  useEffect(() => {
    onSaveClipboardImageRef.current = onSaveClipboardImage;
  }, [onSaveClipboardImage]);

  useEffect(() => {
    onSaveEditorResourcesRef.current = onSaveEditorResources;
  }, [onSaveEditorResources]);

  useEffect(() => {
    onSaveClipboardAttachmentRef.current = onSaveClipboardAttachment;
  }, [onSaveClipboardAttachment]);

  useEffect(() => {
    documentPathRef.current = documentPath;
  }, [documentPath]);

  useEffect(() => {
    onMarkdownChangeRef.current = onMarkdownChange;
  }, [onMarkdownChange]);

  useEffect(() => {
    onActiveOutlineIndexChangeRef.current = onActiveOutlineIndexChange;
  }, [onActiveOutlineIndexChange]);

  useEffect(() => {
    openExternalUrlRef.current = openExternalUrl;
  }, [openExternalUrl]);

  useEffect(() => {
    openLocalAttachmentRef.current = openLocalAttachment;
  }, [openLocalAttachment]);

  useEffect(() => {
    onSaveRemoteClipboardImageRef.current = onSaveRemoteClipboardImage;
  }, [onSaveRemoteClipboardImage]);

  useEffect(() => {
    onTextSelectionChangeRef.current = onTextSelectionChange;
  }, [onTextSelectionChange]);

  useEffect(() => {
    readOnlyRef.current = readOnly;
  }, [readOnly]);

  useEffect(() => {
    resolveImageSrcRef.current = resolveImageSrc;
  }, [resolveImageSrc]);

  useEffect(() => {
    tableColumnWidthModeRef.current = tableColumnWidthMode ?? "auto";
  }, [tableColumnWidthMode]);

  useEffect(() => {
    workspaceFilesRef.current = workspaceFiles ?? [];
  }, [workspaceFiles]);

  useEffect(() => {
    const deferredMarkdownChange = deferredMarkdownChangeRef.current;

    return () => {
      deferredMarkdownChange?.flush();
    };
  }, []);

  const resolveCurrentImageSrc = useCallback((src: string) => {
    return resolveImageSrcRef.current?.(src) ?? src;
  }, []);

  const createEditor = useCallback(
    (root: HTMLElement) => {
      const editor = Editor.make()
        .config((ctx) => {
          ctx.set(rootCtx, root);
          ctx.set(defaultValueCtx, initialContentRef.current);
          ctx.update(remarkStringifyOptionsCtx, (options) => {
            const textHandler = options.handlers?.text;
            if (!textHandler) {
              return {
                ...options,
                bullet: "-" as const
              };
            }

            return {
              ...options,
              bullet: "-" as const,
              handlers: {
                ...options.handlers,
                text: (node, parent, state, info) =>
                  readInlineHtmlMarkdownText(node) ?? textHandler(node, parent, state, info)
              }
            };
          });
          ctx.update(editorViewOptionsCtx, (options) => ({
            ...options,
            attributes: {
              ...options.attributes,
              "aria-label": markdownDocumentLabel,
              "aria-readonly": readOnlyRef.current ? "true" : "false",
              autocomplete: "off",
              autocapitalize: "off",
              autocorrect: "off",
              spellcheck: "false",
              writingsuggestions: "false"
            },
            editable: () => !readOnlyRef.current,
            clipboardTextSerializer: (slice, view) =>
              serializeMarkdownClipboardText(slice, view, ctx.get(serializerCtx))
          }));
          ctx.get(listenerCtx).updated((editorCtx, doc) => {
            try {
              const deferredMarkdownChange = deferredMarkdownChangeRef.current;
              if (!deferredMarkdownChange) return;

              const view = editorCtx.get(editorViewCtx);
              const markdownState = view.state;
              const markdownDocument = markdownState.doc === doc ? doc : markdownState.doc;
              const serializeMarkdown = editorCtx.get(serializerCtx);
              const link = linkSchema.type(editorCtx);
              const image = imageSchema.type(editorCtx);
              const liveMarkdownSpecs = markraLiveMarkdownSpecs(editorCtx, { highlight: highlightSyntaxEnabled });

              // Serializing the whole ProseMirror document on every transaction is the hot path for long files.
              deferredMarkdownChange.schedule(() => {
                try {
                  onMarkdownChangeRef.current(
                    restoreEscapedMarkdownSource(
                      serializeLinkImageLiveMarkdown(markdownDocument, serializeMarkdown, link, image),
                      markdownState,
                      liveMarkdownSpecs,
                      serializeMarkdown
                    )
                  );
                } catch {
                  // Milkdown can flush a delayed update after teardown in tests or fast window closes.
                }
              });
            } catch {
              // Milkdown can flush a delayed update after teardown in tests or fast window closes.
            }
          });
        })
        .use(markraReadOnlyTransactionGuard(readOnlyRef))
        .use(listener)
        .use(history)
        .use(markraFrontmatterRemarkPlugin)
        .use(markraMathRemarkPlugin)
        .use(markraCommonmark)
        .use(markraFrontmatterSchema)
        .use(markraGfm)
        .use(markraFootnoteReferenceInputRule)
        .use(markraFootnoteDefinitionInputPlugin())
        .use(markraTaskListSchema)
        .use(markraTaskListPlugin)
        .use(markraCalloutSerializerPlugin);

      if (githubAlertsEnabled) {
        editor.use(markraCalloutRemarkPlugin);
        editor.use(markraCalloutPlugin);
      }

      editor
        .use(markraSlashCommands(slashCommandLabels, { callout: githubAlertsEnabled }))
        .use(markraMathSourcePlugin)
        .use(markraMarkdownShortcuts(normalizedMarkdownShortcuts))
        .use(markraMarkdownSourcePastePlugin)
        .use(markraCodeBlockPlugin)
        .use(markraMathCaretAnchorSuppressionPlugin)
        .use(markraMathPlugin)
        .use(markraSelectionHoldPlugin)
        .use(markraVisualCaretPlugin)
        .use(markraSearchPlugin())
        .use(markraFootnotePreviewPlugin())
        .use(markraBlockGapPlugin)
        .use(markraBlockDragPlugin(blockDragLabels))
        .use(markraHeadingTogglePlugin(headingToggleLabels))
        .use(markraListTogglePlugin(listToggleLabels))
        .use(
          markraDocumentLinkCompletionPlugin({
            getDocumentPath: () => documentPathRef.current,
            getWorkspaceFiles: () => workspaceFilesRef.current
          })
        )
        .use(
          markraTextSelectionObserverPlugin(
            (selection) => {
              onTextSelectionChangeRef.current?.(selection);
            },
            {
              onActiveOutlineIndexChange: (index) => {
                onActiveOutlineIndexChangeRef.current?.(index);
              }
            }
          )
        )
        .use(markraTableFragmentMergePlugin(tableFragmentMergeLabel))
        .use(markraTableControlsPlugin(tableControlLabels, {
          getDefaultWidthMode: () => tableColumnWidthModeRef.current,
          getDocumentKey: () => documentPathRef.current
        }))
        .use(markraTrailingParagraphPlugin)
        .use(markraLinkImageLivePlugin(resolveCurrentImageSrc))
        .use(markraHeadingLevelPlugin(headingLevelLabels))
        .use(
          markraRawHtmlPlugin({
            htmlSourceApplyLabel: t(language, "editor.htmlSourceApply"),
            htmlSourceLabel: t(language, "editor.htmlSource"),
            resolveImageSrc: resolveCurrentImageSrc
          })
        )
        .use(markraLiveMarkdownPlugin({ highlight: highlightSyntaxEnabled, initialMarkdown: initialContentRef.current }));

      if (externalLinkOpeningEnabled) {
        editor.use(
          markraExternalLinkClickPlugin({
            openExternalUrl: (url) => {
              return openExternalUrlRef.current?.(url);
            },
            openLocalAttachment: (src) => {
              return openLocalAttachmentRef.current?.(src);
            }
          })
        );
      }

      if (
        onSaveEditorResourcesRef.current ||
        onSaveClipboardImageRef.current ||
        onSaveClipboardAttachmentRef.current ||
        onSaveRemoteClipboardImageRef.current
      ) {
        editor.use(
          markraClipboardImagePluginWithOptions(
            (image) => onSaveClipboardImageRef.current?.(image) ?? Promise.resolve(null),
            {
              documentPath: () => documentPathRef.current,
              ...(onSaveEditorResourcesRef.current
                ? {
                    saveResource: (request) => onSaveEditorResourcesRef.current?.(request) ?? Promise.resolve([])
                  }
                : {}),
              saveAttachment: (attachment) => onSaveClipboardAttachmentRef.current?.(attachment) ?? Promise.resolve(null),
              saveRemoteImage: (image) => onSaveRemoteClipboardImageRef.current?.(image) ?? Promise.resolve(null)
            }
          )
        );
      }

      return editor;
    },
    [
      externalLinkOpeningEnabled,
      githubAlertsEnabled,
      highlightSyntaxEnabled,
      language,
      markdownDocumentLabel,
      normalizedMarkdownShortcuts,
      resolveCurrentImageSrc,
      slashCommandLabels
    ]
  );

  useEditor(createEditor, [createEditor]);

  return (
    <>
      <Milkdown />
      <MilkdownReadOnlyBridge readOnly={readOnly} />
      <MilkdownInstanceBridge autoFocus={autoFocus} onEditorReady={onEditorReady} />
    </>
  );
}

export function MarkdownPaperSurface(props: MarkdownPaperSurfaceProps) {
  return (
    <MilkdownProvider>
      <MilkdownEditorSurface {...props} />
    </MilkdownProvider>
  );
}
