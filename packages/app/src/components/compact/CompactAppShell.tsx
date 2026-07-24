import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  compactPagesEqual,
  useCompactNavigation,
  type CompactNavigation,
  type CompactOverlayPage,
  type CompactPage
} from "../../hooks/useCompactNavigation";
import type { AppSystemBackSubscriber } from "../../runtime";
import type { CompactAppController } from "./types";
import { CompactFileBrowserScreen } from "./CompactFileBrowserScreen";
import { CompactMoveTargetScreen } from "./CompactMoveTargetScreen";
import { CompactEditorScreen } from "./CompactEditorScreen";
import {
  CompactSyncFormScreen,
  type CompactSyncFormExitPreparation
} from "./CompactSyncFormScreen";
import { CompactSyncStatusScreen } from "./CompactSyncStatusScreen";
import { CompactSettingsHome } from "./CompactSettingsHome";
import { CompactSettingsDetail } from "./CompactSettingsDetail";
import { useVisualViewport } from "../../hooks/useVisualViewport";

type CompactAppShellProps = {
  controller: CompactAppController;
  onNavigationRequestComplete?: (requestId: number) => unknown;
  onExitSyncForm?: () => Promise<unknown> | unknown;
  onNavigationError?: (error: unknown) => unknown;
  renderPage?: (page: CompactPage, navigation: CompactNavigation) => ReactNode;
  subscribeToSystemBack?: AppSystemBackSubscriber;
};

type CompactShellStyle = CSSProperties & {
  "--compact-keyboard-inset": string;
  "--compact-safe-area-bottom": string;
  "--compact-safe-area-top": string;
  "--compact-visual-viewport-height": string;
};

export function CompactAppShell({
  controller,
  onNavigationRequestComplete,
  onExitSyncForm,
  onNavigationError,
  renderPage,
  subscribeToSystemBack
}: CompactAppShellProps) {
  const viewport = useVisualViewport();
  const [syncExitError, setSyncExitError] = useState(false);
  const shellMountedRef = useRef(true);
  const syncFormEndedRef = useRef(false);
  const syncFormExitAttemptRef = useRef<Promise<unknown> | null>(null);
  const syncFormPrepareExitRef = useRef<(() => Promise<CompactSyncFormExitPreparation>) | null>(null);
  const syncFormTeardownAttemptRef = useRef<Promise<unknown> | null>(null);
  const registerSyncFormBeforeExit = useCallback((prepareExit: () => Promise<CompactSyncFormExitPreparation>) => {
    if (syncFormPrepareExitRef.current !== prepareExit) {
      syncFormPrepareExitRef.current = prepareExit;
      syncFormEndedRef.current = false;
    }
  }, []);
  const endSyncFormSession = useCallback(async () => {
    if (onExitSyncForm) {
      await onExitSyncForm();
    } else {
      await controller.sync.end("category-leave");
    }
  }, [controller.sync.end, onExitSyncForm]);
  const exitSyncForm = useCallback(() => {
    if (syncFormEndedRef.current) return Promise.resolve(undefined);
    const pendingAttempt = syncFormExitAttemptRef.current;
    if (pendingAttempt) return pendingAttempt;
    if (shellMountedRef.current) setSyncExitError(false);

    const attempt = Promise.resolve().then(async () => {
      const preparation = await syncFormPrepareExitRef.current?.();
      if (preparation?.shouldEndSession !== false) {
        await endSyncFormSession();
      }
      syncFormEndedRef.current = true;
    });
    const trackedAttempt = attempt.then((result) => result, (error: unknown) => {
      if (shellMountedRef.current) setSyncExitError(true);
      throw error;
    }).finally(() => {
      if (syncFormExitAttemptRef.current === trackedAttempt) {
        syncFormExitAttemptRef.current = null;
      }
    });
    syncFormExitAttemptRef.current = trackedAttempt;
    return trackedAttempt;
  }, [endSyncFormSession]);
  const releaseSyncFormOnTeardown = useCallback(() => {
    if (syncFormEndedRef.current) return Promise.resolve(undefined);
    const pendingTeardown = syncFormTeardownAttemptRef.current;
    if (pendingTeardown) return pendingTeardown;

    const attempt = Promise.resolve().then(async () => {
      const pendingExit = syncFormExitAttemptRef.current;
      if (pendingExit) {
        try {
          await pendingExit;
        } catch {
          // Teardown must release an active session even when normal navigation is blocked.
        }
        if (syncFormEndedRef.current) return;
      } else {
        try {
          await syncFormPrepareExitRef.current?.();
        } catch {
          // Failed writes block user navigation, but cannot keep a removed shell editing.
        }
      }

      try {
        await endSyncFormSession();
      } catch {
        await endSyncFormSession();
      }
      syncFormEndedRef.current = true;
    });
    const trackedAttempt = attempt.finally(() => {
      if (syncFormTeardownAttemptRef.current === trackedAttempt) {
        syncFormTeardownAttemptRef.current = null;
      }
    });
    syncFormTeardownAttemptRef.current = trackedAttempt;
    return trackedAttempt;
  }, [endSyncFormSession]);
  const releaseSyncFormOnTeardownRef = useRef(releaseSyncFormOnTeardown);
  releaseSyncFormOnTeardownRef.current = releaseSyncFormOnTeardown;
  const navigation = useCompactNavigation({
    onBeforePop: (page) => {
      if (page.kind !== "sync-form") return;
      return exitSyncForm();
    },
    onNavigationError,
    subscribeToSystemBack
  });
  const requestedNavigationRef = useRef<{
    completed: boolean;
    id: number;
    opened: boolean;
    page: CompactOverlayPage;
    retainUntilEditor: boolean;
  } | null>(null);
  useEffect(() => {
    const request = controller.navigationRequest;
    if (!request) {
      requestedNavigationRef.current = null;
      return;
    }

    let trackedRequest = requestedNavigationRef.current;
    if (
      !trackedRequest
      || trackedRequest.id !== request.id
      || !compactPagesEqual(trackedRequest.page, request.page)
      || trackedRequest.retainUntilEditor !== request.retainUntilEditor
    ) {
      trackedRequest = {
        completed: false,
        id: request.id,
        opened: false,
        page: request.page,
        retainUntilEditor: request.retainUntilEditor
      };
      requestedNavigationRef.current = trackedRequest;
    }
    if (trackedRequest.completed) return;

    const completeRequest = () => {
      if (trackedRequest.completed) return;
      trackedRequest.completed = true;
      onNavigationRequestComplete?.(trackedRequest.id);
    };
    if (compactPagesEqual(navigation.page, trackedRequest.page)) {
      trackedRequest.opened = true;
      if (!trackedRequest.retainUntilEditor) completeRequest();
      return;
    }
    if (trackedRequest.opened) {
      if (trackedRequest.retainUntilEditor && navigation.page.kind === "editor") {
        completeRequest();
      }
      return;
    }
    if (!navigation.push(trackedRequest.page)) completeRequest();
  }, [controller.navigationRequest, navigation.page, navigation.push, onNavigationRequestComplete]);
  const currentPageRef = useRef(navigation.page);
  currentPageRef.current = navigation.page;
  useEffect(() => {
    shellMountedRef.current = true;
    return () => {
      shellMountedRef.current = false;
      if (currentPageRef.current.kind === "sync-form") {
        releaseSyncFormOnTeardownRef.current().then(undefined, () => undefined);
      }
    };
  }, []);
  const defaultPageContent = navigation.page.kind === "editor" ? (
    <CompactEditorScreen controller={controller} navigation={navigation} />
  ) : navigation.page.kind === "files" ? (
    <CompactFileBrowserScreen controller={controller} navigation={navigation} />
  ) : navigation.page.kind === "move-target" ? (
    <CompactMoveTargetScreen
      controller={controller}
      navigation={navigation}
      path={navigation.page.path}
    />
  ) : navigation.page.kind === "sync-status" ? (
    <CompactSyncStatusScreen
      controller={controller.sync}
      language={controller.language}
      navigation={navigation}
    />
  ) : navigation.page.kind === "sync-form" ? (
    <CompactSyncFormScreen
      controller={controller.sync}
      exitError={syncExitError}
      language={controller.language}
      mode={navigation.page.mode}
      navigation={navigation}
      registerBeforeExit={registerSyncFormBeforeExit}
    />
  ) : navigation.page.kind === "settings" ? (
    <CompactSettingsHome controller={controller} navigation={navigation} />
  ) : navigation.page.kind === "settings-detail" ? (
    navigation.page.category === "sync" ? (
      <CompactSyncStatusScreen
        controller={controller.sync}
        language={controller.language}
        navigation={navigation}
      />
    ) : (
      <CompactSettingsDetail
        category={navigation.page.category}
        controller={controller}
        navigation={navigation}
      />
    )
  ) : null;
  const pageContent = renderPage
    ? renderPage(navigation.page, navigation)
    : defaultPageContent;
  const shellStyle: CompactShellStyle = {
    "--compact-keyboard-inset": `${viewport.keyboardInset}px`,
    "--compact-safe-area-bottom": "env(safe-area-inset-bottom, 0px)",
    "--compact-safe-area-top": "env(safe-area-inset-top, 0px)",
    "--compact-visual-viewport-height": `${viewport.visualHeight}px`
  };
  const editorCovered = navigation.page.kind !== "editor";

  return (
    <main
      className="compact-app-shell relative h-full w-full overflow-hidden overscroll-none bg-(--bg-primary) text-(--text-primary)"
      data-compact="true"
      data-testid="compact-app-shell"
      style={shellStyle}
    >
      <div
        className="relative z-0 h-full min-h-0 overflow-hidden"
        data-compact-editor-layer
        aria-hidden={editorCovered ? true : undefined}
        inert={editorCovered ? true : undefined}
      >
        {renderPage ? (
          <>
            {controller.editor.host}
            {navigation.page.kind === "editor" ? pageContent : null}
          </>
        ) : <CompactEditorScreen controller={controller} navigation={navigation} />}
      </div>
      {navigation.page.kind === "editor" ? null : (
        <div
          className="absolute inset-0 z-10 h-full min-h-0 w-full overflow-hidden bg-(--bg-primary)"
          data-compact-page={navigation.page.kind}
        >
          {pageContent}
        </div>
      )}
    </main>
  );
}
