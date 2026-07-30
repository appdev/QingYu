import { createUnavailableNativeShellPort } from "./native-shell";
import type {
  NativeAbsolutePath,
  NativeShellPort,
  NativeStandaloneDocumentHandle,
  NativeStandaloneWriteInput,
} from "./native-shell";

type WorkspaceMutationKey =
  | "createDocument"
  | "deleteDocument"
  | "moveDocument"
  | "triggerSync"
  | "updateDocument"
  | "writeWorkspaceDocument";

describe("NativeShellPort", () => {
  it("keeps absolute paths and standalone handles opaque", () => {
    expectTypeOf<string>().not.toMatchTypeOf<NativeAbsolutePath>();
    expectTypeOf<string>().not.toMatchTypeOf<NativeStandaloneDocumentHandle>();
  });

  it("cannot express a workspace-domain mutation through the standalone surface", () => {
    expectTypeOf<Extract<keyof NativeShellPort["standalone"], WorkspaceMutationKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof NativeStandaloneWriteInput, "absolutePath" | "workspaceGeneration">>()
      .toEqualTypeOf<never>();
    expectTypeOf<NativeStandaloneWriteInput>().toMatchTypeOf<{
      contents: string;
      handle: NativeStandaloneDocumentHandle;
    }>();
    expectTypeOf<NativeShellPort["standalone"]["release"]>().toMatchTypeOf<
      (handle: NativeStandaloneDocumentHandle) => Promise<unknown>
    >();
  });

  it("reports every native capability unavailable and rejects instead of emulating a cancel", async () => {
    const port = createUnavailableNativeShellPort();

    expect(port.capabilities).toEqual({
      absolutePathClassification: "unavailable",
      operatingSystemShell: "unavailable",
      pickers: "unavailable",
      standaloneDocuments: "unavailable",
    });
    await expect(port.pickers.pickWorkspaceDirectory()).rejects.toMatchObject({
      name: "NativeShellUnavailableError",
    });
    await expect(port.paths.classify("/notes" as NativeAbsolutePath)).rejects.toMatchObject({
      name: "NativeShellUnavailableError",
    });
    await expect(port.standalone.release(
      "opaque-handle" as NativeStandaloneDocumentHandle,
    )).rejects.toMatchObject({
      name: "NativeShellUnavailableError",
    });
  });
});
