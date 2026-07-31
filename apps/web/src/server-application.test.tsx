import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import type { AppRuntime, KernelDomainPort } from "@markra/app/runtime";

import {
  ServerStartupShell,
  startServerWebApplication,
} from "./server-application";
import type {
  ServerWebBootstrapOwner,
  ServerWebBootstrapSnapshot,
} from "./server-bootstrap";
import { resolveServerStartupLanguage } from "./server-startup-language";

describe("server Web application gate", () => {
  afterEach(() => cleanup());

  it("never configures or renders App before the authenticated runtime is ready", async () => {
    const snapshots: ServerWebBootstrapSnapshot[] = [{ phase: "checking" }];
    const subscribers = new Set<(snapshot: ServerWebBootstrapSnapshot) => unknown>();
    const owner = {
      start: vi.fn(async () => undefined),
      subscribe: vi.fn((subscriber: (snapshot: ServerWebBootstrapSnapshot) => unknown) => {
        subscribers.add(subscriber);
        subscriber(snapshots.at(-1)!);
        return () => {
          subscribers.delete(subscriber);
          return undefined;
        };
      }),
      getSnapshot: () => snapshots.at(-1)!,
      close: vi.fn(() => undefined),
      initialize: vi.fn(async () => undefined),
      login: vi.fn(async () => undefined),
      retry: vi.fn(async () => undefined),
    } satisfies ServerWebBootstrapOwner;
    const configureRuntime = vi.fn();
    const renderApp = vi.fn();
    const renderStartup = vi.fn();
    const kernel = {} as KernelDomainPort;
    const runtime = { kernel } as AppRuntime;
    const releaseRuntime = vi.fn(() => undefined);

    const stop = startServerWebApplication({
      configureRuntime,
      createRuntime: vi.fn(() => ({ runtime, release: releaseRuntime })),
      owner,
      renderApp,
      renderStartup,
    });
    await owner.start();

    expect(renderStartup).toHaveBeenCalledWith({ phase: "checking" }, owner);
    expect(configureRuntime).not.toHaveBeenCalled();
    expect(renderApp).not.toHaveBeenCalled();

    const ready: ServerWebBootstrapSnapshot = {
      phase: "ready",
      result: {
        kernel,
        runtime: {
          capabilities: {
            documents: true, history: true, portableSettings: true,
            resources: true, s3: true, search: true, settings: true,
            sync: true, webdav: true,
          },
          instanceId: "123e4567-e89b-42d3-a456-426614174000",
          profile: "server",
          startupState: "ready",
        },
        workspace: {
          displayName: "Notes",
          generation: "generation-1" as never,
          id: "workspace-1",
          readiness: "ready",
          revision: "revision-1" as never,
        },
      },
    };
    snapshots.push(ready);
    for (const subscriber of subscribers) subscriber(ready);

    expect(configureRuntime).toHaveBeenCalledWith(runtime);
    expect(renderApp).toHaveBeenCalledOnce();
    stop();
    expect(releaseRuntime).toHaveBeenCalledOnce();
    expect(owner.close).toHaveBeenCalledOnce();
  });

  it("releases browser runtime owners on authentication loss, Kernel replacement, and stop", () => {
    let subscriber: ((snapshot: ServerWebBootstrapSnapshot) => unknown) | undefined;
    const owner = {
      ...inertOwner(),
      close: vi.fn(() => undefined),
      subscribe: vi.fn((next: (snapshot: ServerWebBootstrapSnapshot) => unknown) => {
        subscriber = next;
        next({ phase: "checking" });
        return () => {
          subscriber = undefined;
          return undefined;
        };
      }),
    } satisfies ServerWebBootstrapOwner;
    const kernelA = {} as KernelDomainPort;
    const kernelB = {} as KernelDomainPort;
    const releaseA1 = vi.fn(() => undefined);
    const releaseA2 = vi.fn(() => undefined);
    const releaseB = vi.fn(() => undefined);
    const runtimeA1 = { kernel: kernelA } as AppRuntime;
    const runtimeA2 = { kernel: kernelA } as AppRuntime;
    const runtimeB = { kernel: kernelB } as AppRuntime;
    const runtimeOwners = [
      { runtime: runtimeA1, release: releaseA1 },
      { runtime: runtimeA2, release: releaseA2 },
      { runtime: runtimeB, release: releaseB },
    ];
    const createRuntime = vi.fn(() => {
      const runtimeOwner = runtimeOwners.shift();
      if (runtimeOwner === undefined) throw new Error("Unexpected runtime replacement.");
      return runtimeOwner;
    });
    const configureRuntime = vi.fn();
    const stop = startServerWebApplication({
      configureRuntime,
      createRuntime,
      owner,
      renderApp: vi.fn(),
      renderStartup: vi.fn(),
    });

    subscriber?.(readySnapshot(kernelA));
    subscriber?.(readySnapshot(kernelA));
    expect(createRuntime).toHaveBeenCalledOnce();

    subscriber?.({ phase: "login", error: null });
    expect(releaseA1).toHaveBeenCalledOnce();

    subscriber?.(readySnapshot(kernelA));
    subscriber?.(readySnapshot(kernelB));
    expect(releaseA2).toHaveBeenCalledOnce();
    expect(releaseA2.mock.invocationCallOrder[0])
      .toBeLessThan(configureRuntime.mock.invocationCallOrder[2]!);

    stop();
    stop();
    expect(releaseB).toHaveBeenCalledOnce();
    expect(releaseB.mock.invocationCallOrder[0])
      .toBeLessThan(owner.close.mock.invocationCallOrder[0]!);
    expect(owner.close).toHaveBeenCalledOnce();
  });

  it("renders only fixed safe startup copy and never embeds credential values", () => {
    const owner = inertOwner();
    const initialization = renderToStaticMarkup(
      <ServerStartupShell
        owner={owner}
        snapshot={{ phase: "initialize", error: null }}
      />,
    );
    expect(initialization).toContain("Set up this server");
    expect(initialization).toContain("type=\"password\"");
    expect(initialization).not.toContain("value=");

    const limited = renderToStaticMarkup(
      <ServerStartupShell
        owner={owner}
        snapshot={{
          phase: "login",
          error: { kind: "rate-limited", retryAfterSeconds: 31 },
        }}
      />,
    );
    expect(limited).toContain("31 seconds");
  });

  it("renders the approved Chinese login copy without the removed subtitles", () => {
    const markup = renderToStaticMarkup(
      <ServerStartupShell
        language="zh-CN"
        owner={inertOwner()}
        serverAddress="192.168.0.172:3210"
        snapshot={{ phase: "login", error: null }}
        transport="HTTP"
      />,
    );

    expect(markup).toContain("明窗净几，");
    expect(markup).toContain("字字轻语。");
    expect(markup).toContain("欢迎回来");
    expect(markup).toContain("服务器密码");
    expect(markup).toContain("192.168.0.172:3210");
    expect(markup).toContain("/favicon.png");
    expect(markup).not.toContain("输入服务器密码，继续写作");
    expect(markup).not.toContain("笔记始终是普通的 Markdown 文件");
    expect(markup).not.toContain("密码只会发送到当前服务器");
  });

  it("reveals and conceals a password without retaining it in component state", () => {
    render(
      <ServerStartupShell
        language="zh-CN"
        owner={inertOwner()}
        snapshot={{ phase: "login", error: null }}
      />,
    );
    const input = screen.getByLabelText("服务器密码") as HTMLInputElement;
    const reveal = screen.getByRole("button", { name: "显示密码" });

    expect(input.type).toBe("password");
    fireEvent.click(reveal);
    expect(input.type).toBe("text");
    expect(
      screen.getByRole("button", { name: "隐藏密码" }).getAttribute("aria-pressed"),
    ).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "隐藏密码" }));
    expect(input.type).toBe("password");
  });

  it("distinguishes pointer focus from keyboard focus for the input treatment", () => {
    render(
      <ServerStartupShell
        owner={inertOwner()}
        snapshot={{ phase: "login", error: null }}
      />,
    );

    fireEvent.pointerDown(document);
    expect(document.body.dataset.serverFocusOrigin).toBe("pointer");
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.body.dataset.serverFocusOrigin).toBe("keyboard");
  });

  it("submits login secrets to the owner and clears the field", () => {
    const owner = inertOwner();
    owner.login = vi.fn(async () => undefined);
    render(
      <ServerStartupShell
        owner={owner}
        snapshot={{ phase: "login", error: null }}
      />,
    );
    const input = screen.getByLabelText("Server password") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "owner-secret" } });
    fireEvent.submit(input.form!);

    expect(owner.login).toHaveBeenCalledWith({ password: "owner-secret" });
    expect(input.value).toBe("");
  });

  it("blocks mismatched initialization passwords and submits only matching secrets", () => {
    const owner = inertOwner();
    owner.initialize = vi.fn(async () => undefined);
    render(
      <ServerStartupShell
        language="zh-CN"
        owner={owner}
        snapshot={{ phase: "initialize", error: null }}
      />,
    );
    const token = screen.getByLabelText("一次性初始化令牌") as HTMLInputElement;
    const password = screen.getByLabelText("所有者密码") as HTMLInputElement;
    const confirmation = screen.getByLabelText("确认密码") as HTMLInputElement;
    fireEvent.change(token, { target: { value: "one-time-token" } });
    fireEvent.change(password, { target: { value: "twelve-chars-1" } });
    fireEvent.change(confirmation, { target: { value: "twelve-chars-2" } });
    fireEvent.submit(token.form!);

    expect(owner.initialize).not.toHaveBeenCalled();
    expect(screen.getByText("两次密码不一致。请再次输入相同的密码。")).not.toBeNull();
    expect(confirmation.getAttribute("aria-invalid")).toBe("true");

    fireEvent.change(confirmation, { target: { value: "twelve-chars-1" } });
    fireEvent.submit(token.form!);
    expect(owner.initialize).toHaveBeenCalledWith({
      initializationToken: "one-time-token",
      password: "twelve-chars-1",
    });
    expect(token.value).toBe("");
    expect(password.value).toBe("");
    expect(confirmation.value).toBe("");
  });

  it("resolves Chinese from an explicit startup language or browser preference", () => {
    expect(resolveServerStartupLanguage("?startupLanguage=zh-CN", ["en-US"])).toBe("zh-CN");
    expect(resolveServerStartupLanguage("", ["zh-Hans-CN", "en-US"])).toBe("zh-CN");
    expect(resolveServerStartupLanguage("?startupLanguage=en", ["zh-CN"])).toBe("en");
    expect(resolveServerStartupLanguage("", ["fr-FR"])).toBe("en");
  });
});

function inertOwner(): ServerWebBootstrapOwner {
  return {
    close: () => undefined,
    getSnapshot: () => ({ phase: "checking" }),
    initialize: async () => undefined,
    login: async () => undefined,
    retry: async () => undefined,
    start: async () => undefined,
    subscribe: () => () => undefined,
  };
}

function readySnapshot(kernel: KernelDomainPort): ServerWebBootstrapSnapshot {
  return {
    phase: "ready",
    result: {
      kernel,
      runtime: {
        capabilities: {
          documents: true, history: true, portableSettings: true,
          resources: true, s3: true, search: true, settings: true,
          sync: true, webdav: true,
        },
        instanceId: "123e4567-e89b-42d3-a456-426614174000",
        profile: "server",
        startupState: "ready",
      },
      workspace: {
        displayName: "Notes",
        generation: "generation-1" as never,
        id: "workspace-1",
        readiness: "ready",
        revision: "revision-1" as never,
      },
    },
  };
}
