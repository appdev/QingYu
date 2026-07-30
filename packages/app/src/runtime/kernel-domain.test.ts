import { createUnavailableKernelDomainPort } from "./kernel-domain";
import type {
  KernelDocumentLocator,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelRevision,
  KernelRuntimeSnapshot,
  KernelUpdateDocumentInput,
  KernelWorkspaceGeneration,
  KernelWorkspaceSnapshot,
} from "./kernel-domain";

type ForbiddenHostKey =
  | "absolutePath"
  | "endpoint"
  | "host"
  | "origin"
  | "port"
  | "rootPath"
  | "token";

describe("KernelDomainPort", () => {
  it("keeps application DTOs free of host and absolute-path fields", () => {
    expectTypeOf<Extract<keyof KernelRuntimeSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelWorkspaceSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelDocumentSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
  });

  it("requires opaque locators and optimistic workspace/document revisions for updates", () => {
    expectTypeOf<string>().not.toMatchTypeOf<KernelDocumentLocator>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelWorkspaceGeneration>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelRevision>();
    expectTypeOf<Parameters<KernelDomainPort["documents"]["update"]>[0]>()
      .toEqualTypeOf<KernelUpdateDocumentInput>();
    expectTypeOf<KernelUpdateDocumentInput>().toMatchTypeOf<{
      contents: string;
      expectedRevision: KernelRevision;
      locator: KernelDocumentLocator;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
  });

  it("fails closed when no Kernel adapter is installed", async () => {
    const port = createUnavailableKernelDomainPort();

    expect(port.availability).toBe("unavailable");
    await expect(port.runtime.read()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.workspace.read()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
  });
});
