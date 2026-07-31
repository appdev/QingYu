import type { KernelDomainPort } from "@markra/app/runtime";
import { listen } from "@tauri-apps/api/event";

import {
  createNativeKernelBootstrapLifecycleOwner,
  type NativeKernelBootstrap,
  type NativeKernelBootstrapLifecycleSnapshot,
  type NativeKernelBootstrapInvoke
} from "../kernel-bootstrap";
import {
  createDesktopKernelDomainAdapter,
  type DesktopKernelDomainAdapter,
  type DesktopKernelDomainAdapterOptions
} from "./kernel";
import {
  createDesktopKernelEventsAdapter,
  type DesktopKernelDomainInvalidation,
  type DesktopKernelEventsAdapter,
  type DesktopKernelEventsAdapterOptions,
  type DesktopKernelEventsErrorNotice,
  type DesktopKernelEventsStateNotice
} from "./kernel-events";
import {
  createDesktopKernelInvalidationBridge,
  type DesktopKernelInvalidationBridge
} from "./kernel-invalidations";

export const NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT = "qingyu://kernel-bootstrap-changed";

export type NativeKernelBootstrapChangedListener = (
  eventName: typeof NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT,
  handler: (event: unknown) => unknown
) => Promise<() => unknown>;

export type NativeKernelPagehideListener = (
  handler: () => unknown
) => () => unknown;

export type NativeKernelSessionSnapshot =
  | {
      readonly domain: null;
      readonly status: "dormant";
      readonly generation?: string;
    }
  | {
      readonly domain: null;
      readonly status: "starting" | "retrying" | "failed";
      readonly generation: string;
    }
  | {
      readonly domain: KernelDomainPort;
      readonly status: "ready";
      readonly generation: string;
      readonly instanceId: string;
    };

export interface NativeKernelSessionOwner {
  start(): Promise<undefined>;
  subscribe(
    subscriber: (snapshot: NativeKernelSessionSnapshot | null) => unknown
  ): () => undefined;
  getSnapshot(): NativeKernelSessionSnapshot | null;
  close(): undefined;
}

export interface NativeKernelSessionOwnerOptions {
  readonly invokeCommand?: NativeKernelBootstrapInvoke;
  readonly listenBootstrapChanged?: NativeKernelBootstrapChangedListener;
  readonly addPagehideListener?: NativeKernelPagehideListener;
  readonly createDomainAdapter?: (
    bootstrap: NativeKernelBootstrap,
    options?: DesktopKernelDomainAdapterOptions
  ) => Promise<DesktopKernelDomainAdapter>;
  readonly createEventsAdapter?: (
    options: DesktopKernelEventsAdapterOptions
  ) => DesktopKernelEventsAdapter;
  readonly onInvalidation?: (invalidation: DesktopKernelDomainInvalidation) => unknown;
  readonly onEventsStateChange?: (notice: DesktopKernelEventsStateNotice) => unknown;
  readonly onEventsError?: (notice: DesktopKernelEventsErrorNotice) => unknown;
  readonly onError?: (error: Error) => unknown;
}

type NativeKernelAdoptionPhase =
  | "adopting"
  | "flushing"
  | "committed"
  | "retired";

type NativeKernelAdoptionNotice =
  | {
      readonly kind: "error";
      readonly value: DesktopKernelEventsErrorNotice;
    }
  | {
      readonly kind: "invalidation";
      readonly value: DesktopKernelDomainInvalidation;
    }
  | {
      readonly kind: "state";
      readonly value: DesktopKernelEventsStateNotice;
    };

interface NativeKernelAdoptionToken {
  readonly epoch: number;
  readonly generation: string;
  readonly instanceId: string;
  readonly pending: NativeKernelAdoptionNotice[];
  phase: NativeKernelAdoptionPhase;
}

export function createNativeKernelSessionOwner(
  {
    addPagehideListener = addBrowserPagehideListener,
    createDomainAdapter = createDesktopKernelDomainAdapter,
    createEventsAdapter = createDesktopKernelEventsAdapter,
    invokeCommand,
    listenBootstrapChanged = listenForNativeKernelBootstrapChange,
    onError,
    onEventsError,
    onEventsStateChange,
    onInvalidation
  }: NativeKernelSessionOwnerOptions = {}
): NativeKernelSessionOwner {
  const bootstrapOwner = invokeCommand === undefined
    ? createNativeKernelBootstrapLifecycleOwner()
    : createNativeKernelBootstrapLifecycleOwner({ invokeCommand });
  const subscribers = new Set<
    (snapshot: NativeKernelSessionSnapshot | null) => unknown
  >();
  let closed = false;
  let snapshot: NativeKernelSessionSnapshot | null = null;
  let startPromise: Promise<undefined> | undefined;
  let refreshPromise: Promise<undefined> | undefined;
  let refreshRequestSequence = 0;
  let refreshCompletedSequence = 0;
  let stopBootstrapListener: (() => unknown) | undefined;
  let stopPagehideListener: (() => unknown) | undefined;
  let activeDomain: DesktopKernelDomainAdapter | undefined;
  let activeEvents: DesktopKernelEventsAdapter | undefined;
  let activeInvalidations: DesktopKernelInvalidationBridge | undefined;
  let activeIdentity:
    | { readonly generation: string; readonly instanceId: string }
    | undefined;
  let activePublication:
    | {
        readonly delivered: Set<(
          snapshot: NativeKernelSessionSnapshot | null
        ) => unknown>;
        readonly next: NativeKernelSessionSnapshot | null;
      }
    | undefined;
  let adoptionEpoch = 0;
  let publicationEpoch = 0;

  const publish = (next: NativeKernelSessionSnapshot | null) => {
    if (closed) {
      snapshot = null;
      return undefined;
    }
    publicationEpoch += 1;
    const publication = publicationEpoch;
    snapshot = next;
    const delivery = {
      delivered: new Set<(snapshot: NativeKernelSessionSnapshot | null) => unknown>(),
      next
    };
    activePublication = delivery;
    try {
      for (const subscriber of [...subscribers]) {
        if (
          closed ||
          publication !== publicationEpoch ||
          snapshot !== next
        ) break;
        delivery.delivered.add(subscriber);
        notifyConsumer(subscriber, next);
      }
    } finally {
      if (activePublication === delivery) activePublication = undefined;
    }
    return undefined;
  };

  const detachActive = () => {
    activeIdentity = undefined;
    publicationEpoch += 1;
    snapshot = null;
    const domain = activeDomain;
    const events = activeEvents;
    const invalidations = activeInvalidations;
    activeDomain = undefined;
    activeEvents = undefined;
    activeInvalidations = undefined;
    return { domain, events, invalidations };
  };

  const releaseDetached = (
    { domain, events, invalidations }: ReturnType<typeof detachActive>
  ) => {
    safelyCall(invalidations?.close);
    safelyCall(domain?.release);
    safelyCall(events?.close);
    return undefined;
  };

  const failClosed = (cause: unknown) => {
    adoptionEpoch += 1;
    const detached = detachActive();
    publish(null);
    releaseDetached(detached);
    return cause;
  };

  const isCurrentIdentity = (
    { generation, instanceId }: {
      readonly generation: string;
      readonly instanceId: string;
    }
  ) => {
    return (
      !closed &&
      activeIdentity?.generation === generation &&
      activeIdentity.instanceId === instanceId
    );
  };

  const matchesAdoptionIdentity = (
    token: NativeKernelAdoptionToken,
    identity: { readonly generation: string; readonly instanceId: string }
  ) => {
    return (
      token.generation === identity.generation &&
      token.instanceId === identity.instanceId &&
      token.epoch === adoptionEpoch &&
      !closed
    );
  };

  const isCurrentAdoption = (
    token: NativeKernelAdoptionToken,
    identity: { readonly generation: string; readonly instanceId: string }
  ) => {
    return (
      token.phase === "committed" &&
      matchesAdoptionIdentity(token, identity) &&
      isCurrentIdentity(identity)
    );
  };

  const retireAdoption = (token: NativeKernelAdoptionToken) => {
    token.phase = "retired";
    token.pending.length = 0;
    return undefined;
  };

  const reconcile = async () => {
    let update;
    try {
      update = await bootstrapOwner.refresh();
    } catch (cause: unknown) {
      throw failClosed(cause);
    }
    if (closed) return undefined;

    const lifecycle = update.snapshot;
    if (lifecycle.status !== "ready") {
      if (update.changed || snapshot?.status === "ready") {
        adoptionEpoch += 1;
        const detached = detachActive();
        publish(nonReadySnapshot(lifecycle));
        releaseDetached(detached);
      }
      return undefined;
    }

    const sameActivePublication =
      !update.changed &&
      activeDomain !== undefined &&
      activeInvalidations?.source.available === true &&
      activeEvents?.identity?.generation === lifecycle.generation &&
      activeEvents.identity.instanceId === lifecycle.instanceId &&
      activeIdentity?.generation === lifecycle.generation &&
      activeIdentity.instanceId === lifecycle.instanceId;
    if (sameActivePublication) return undefined;

    adoptionEpoch += 1;
    const adoption = adoptionEpoch;
    const detached = detachActive();
    if (detached.domain !== undefined || detached.events !== undefined) {
      publish(null);
    }
    releaseDetached(detached);
    if (closed || adoption !== adoptionEpoch) return undefined;
    const acquiredDomainLease = bootstrapOwner.acquireReady();
    const acquiredEventsLease = bootstrapOwner.acquireReady();
    if (acquiredDomainLease === null || acquiredEventsLease === null) {
      safelyRelease(acquiredDomainLease);
      safelyRelease(acquiredEventsLease);
      throw failClosed(new Error("native Kernel session publication unavailable"));
    }
    const domainLease = onceOwnedBootstrap(acquiredDomainLease);
    const eventsLease = onceOwnedBootstrap(acquiredEventsLease);
    const invalidations = createDesktopKernelInvalidationBridge();

    let domain: DesktopKernelDomainAdapter;
    try {
      domain = await createDomainAdapter(domainLease, {
        invalidations: invalidations.source
      });
    } catch (cause: unknown) {
      invalidations.close();
      safelyRelease(domainLease);
      safelyRelease(eventsLease);
      throw failClosed(cause);
    }
    if (closed || adoption !== adoptionEpoch) {
      invalidations.close();
      safelyCall(domain.release);
      safelyRelease(eventsLease);
      return undefined;
    }

    let events: DesktopKernelEventsAdapter;
    const adoptionToken: NativeKernelAdoptionToken = {
      epoch: adoption,
      generation: lifecycle.generation,
      instanceId: lifecycle.instanceId,
      pending: [],
      phase: "adopting"
    };
    const deliverAdoptionNotice = (notification: NativeKernelAdoptionNotice) => {
      if (!matchesAdoptionIdentity(adoptionToken, notification.value)) {
        return undefined;
      }
      if (notification.kind === "error") {
        notifyConsumer(onEventsError, notification.value);
      } else if (notification.kind === "invalidation") {
        invalidations.publish(notification.value);
        if (!matchesAdoptionIdentity(adoptionToken, notification.value)) {
          return undefined;
        }
        notifyConsumer(onInvalidation, notification.value);
      } else {
        notifyConsumer(onEventsStateChange, notification.value);
      }
      return undefined;
    };
    const queueOrDeliverAdoptionNotice = (
      notification: NativeKernelAdoptionNotice
    ) => {
      if (!matchesAdoptionIdentity(adoptionToken, notification.value)) {
        return undefined;
      }
      if (
        adoptionToken.phase === "adopting" ||
        adoptionToken.phase === "flushing"
      ) {
        adoptionToken.pending.push(notification);
      } else if (isCurrentAdoption(adoptionToken, notification.value)) {
        deliverAdoptionNotice(notification);
      }
      return undefined;
    };
    try {
      events = createEventsAdapter({
        onError: (notice) => {
          queueOrDeliverAdoptionNotice({ kind: "error", value: notice });
          return undefined;
        },
        onInvalidation: (invalidation) => {
          queueOrDeliverAdoptionNotice({
            kind: "invalidation",
            value: invalidation
          });
          return undefined;
        },
        onStateChange: (notice) => {
          queueOrDeliverAdoptionNotice({ kind: "state", value: notice });
          return undefined;
        }
      });
    } catch (cause: unknown) {
      retireAdoption(adoptionToken);
      invalidations.close();
      safelyCall(domain.release);
      safelyRelease(eventsLease);
      throw failClosed(cause);
    }
    if (closed || adoption !== adoptionEpoch) {
      retireAdoption(adoptionToken);
      invalidations.close();
      safelyCall(domain.release);
      safelyCall(events.close);
      safelyRelease(eventsLease);
      return undefined;
    }

    try {
      events.replaceConnection(eventsLease);
    } catch (cause: unknown) {
      retireAdoption(adoptionToken);
      invalidations.close();
      safelyCall(domain.release);
      safelyCall(events.close);
      safelyRelease(eventsLease);
      throw failClosed(cause);
    }
    if (closed || adoption !== adoptionEpoch) {
      retireAdoption(adoptionToken);
      invalidations.close();
      safelyCall(domain.release);
      safelyCall(events.close);
      safelyRelease(eventsLease);
      return undefined;
    }
    activeDomain = domain;
    activeEvents = events;
    activeInvalidations = invalidations;
    activeIdentity = Object.freeze({
      generation: lifecycle.generation,
      instanceId: lifecycle.instanceId
    });
    adoptionToken.phase = "flushing";
    const readySnapshot = Object.freeze({
      domain: domain.port,
      generation: lifecycle.generation,
      instanceId: lifecycle.instanceId,
      status: "ready" as const
    });
    const readyPublication = publicationEpoch + 1;
    publish(readySnapshot);
    let pendingIndex = 0;
    while (pendingIndex < adoptionToken.pending.length) {
      if (
        adoptionToken.phase !== "flushing" ||
        adoptionToken.epoch !== adoptionEpoch ||
        readyPublication !== publicationEpoch ||
        snapshot !== readySnapshot ||
        !isCurrentIdentity(adoptionToken)
      ) {
        retireAdoption(adoptionToken);
        return undefined;
      }
      const notification = adoptionToken.pending[pendingIndex];
      pendingIndex += 1;
      if (notification !== undefined) deliverAdoptionNotice(notification);
    }
    adoptionToken.pending.length = 0;
    if (
      adoptionToken.phase === "flushing" &&
      adoptionToken.epoch === adoptionEpoch &&
      readyPublication === publicationEpoch &&
      snapshot === readySnapshot &&
      isCurrentIdentity(adoptionToken)
    ) {
      adoptionToken.phase = "committed";
    } else {
      retireAdoption(adoptionToken);
    }
    return undefined;
  };

  const requestRefresh = () => {
    if (closed) return Promise.reject(sessionClosed());
    refreshRequestSequence += 1;
    if (refreshPromise !== undefined) return refreshPromise;

    const pending = (async () => {
      while (refreshCompletedSequence < refreshRequestSequence && !closed) {
        const request = refreshRequestSequence;
        try {
          await reconcile();
          refreshCompletedSequence = request;
        } catch (cause: unknown) {
          refreshCompletedSequence = request;
          if (closed) return undefined;
          if (refreshRequestSequence === request) throw cause;
        }
      }
      return undefined;
    })();
    refreshPromise = pending;
    pending.then(
      () => {
        if (refreshPromise === pending) refreshPromise = undefined;
        return undefined;
      },
      () => {
        if (refreshPromise === pending) refreshPromise = undefined;
        return undefined;
      }
    );
    return pending;
  };

  const reportRefreshFailure = (cause: unknown) => {
    notifyConsumer(onError, safeSessionError(cause));
    return undefined;
  };

  const close = () => {
    if (closed) return undefined;
    const unavailablePublication = activePublication?.next === null
      ? activePublication
      : undefined;
    const notifyUnavailable = snapshot !== null || unavailablePublication !== undefined;
    const unavailableSubscribers = unavailablePublication === undefined
      ? [...subscribers]
      : [...subscribers].filter(
        (subscriber) => !unavailablePublication.delivered.has(subscriber)
      );
    closed = true;
    adoptionEpoch += 1;
    const detached = detachActive();
    if (notifyUnavailable) {
      for (const subscriber of unavailableSubscribers) {
        notifyConsumer(subscriber, null);
      }
    }
    subscribers.clear();
    releaseDetached(detached);
    bootstrapOwner.close();
    safelyCall(stopBootstrapListener);
    safelyCall(stopPagehideListener);
    stopBootstrapListener = undefined;
    stopPagehideListener = undefined;
    return undefined;
  };

  const start = () => {
    if (closed) return Promise.reject(sessionClosed());
    if (startPromise !== undefined) return startPromise;

    stopPagehideListener = once(addPagehideListener(close));
    const pending = (async () => {
      const stop = await listenBootstrapChanged(
        NATIVE_KERNEL_BOOTSTRAP_CHANGED_EVENT,
        () => requestRefresh().catch(reportRefreshFailure)
      );
      if (closed) {
        safelyCall(stop);
        return undefined;
      }
      stopBootstrapListener = once(stop);
      await requestRefresh();
      return undefined;
    })();
    startPromise = pending;
    return pending;
  };

  return Object.freeze({
    start,
    subscribe: (subscriber: (
      snapshot: NativeKernelSessionSnapshot | null
    ) => unknown) => {
      if (closed) throw sessionClosed();
      subscribers.add(subscriber);
      if (snapshot !== null) notifyConsumer(subscriber, snapshot);
      if (!closed) start().catch(reportRefreshFailure);
      let subscribed = true;
      return () => {
        if (!subscribed) return undefined;
        subscribed = false;
        subscribers.delete(subscriber);
        return undefined;
      };
    },
    getSnapshot: () => snapshot,
    close
  });
}

const listenForNativeKernelBootstrapChange: NativeKernelBootstrapChangedListener =
  async (eventName, handler) => {
    const stop = await listen(eventName, () => handler(undefined));
    return stop;
  };

const addBrowserPagehideListener: NativeKernelPagehideListener = (handler) => {
  const listener = () => handler();
  window.addEventListener("pagehide", listener, { once: true });
  return () => {
    window.removeEventListener("pagehide", listener);
    return undefined;
  };
};

function nonReadySnapshot(
  lifecycle: Exclude<NativeKernelBootstrapLifecycleSnapshot, { readonly status: "ready" }>
): NativeKernelSessionSnapshot {
  return Object.freeze({ ...lifecycle, domain: null });
}

function notifyConsumer<T>(
  consumer: ((value: T) => unknown) | undefined,
  value: T
): undefined {
  try {
    consumer?.(value);
  } catch {
    // Consumer failures never interrupt credential or connection ownership cleanup.
  }
  return undefined;
}

function once(operation: () => unknown): () => undefined {
  let active = true;
  return () => {
    if (!active) return undefined;
    active = false;
    safelyCall(operation);
    return undefined;
  };
}

function safelyCall(operation: (() => unknown) | undefined): undefined {
  try {
    operation?.();
  } catch {
    // Native listener, adapter, and credential retirement are best-effort.
  }
  return undefined;
}

function safelyRelease(bootstrap: NativeKernelBootstrap | null): undefined {
  safelyCall(bootstrap?.release);
  return undefined;
}

function onceOwnedBootstrap(bootstrap: NativeKernelBootstrap): NativeKernelBootstrap {
  return Object.freeze({
    ...bootstrap,
    release: once(bootstrap.release)
  });
}

function safeSessionError(cause: unknown): Error {
  if (
    cause instanceof Error &&
    (cause.message === "invalid native Kernel bootstrap" ||
      cause.message === "native Kernel bootstrap generation regressed" ||
      cause.message === "native Kernel bootstrap refresh failed" ||
      cause.message === "native Kernel bootstrap lifecycle owner closed")
  ) {
    return cause;
  }
  return new Error("native Kernel session refresh failed");
}

function sessionClosed(): Error {
  return new Error("native Kernel session owner closed");
}
