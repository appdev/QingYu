import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeStore
} from "../../runtime";
import { consumeWelcomeDocumentState, resetWelcomeDocumentState } from "./local-state";

describe("temporary welcome document state", () => {
  const values = new Map<string, unknown>();
  const get = vi.fn(async (key: string) => values.get(key));
  const store: RuntimeStore = {
    delete: vi.fn(async (key) => values.delete(key)),
    get: async <T,>(key: string) => get(key) as Promise<T | undefined>,
    save: vi.fn(async () => undefined),
    set: vi.fn(async (key, value) => values.set(key, value))
  };
  const loadStore = vi.fn(async () => store);

  beforeEach(() => {
    values.clear();
    vi.clearAllMocks();
    configureAppRuntime({
      ...createDefaultAppRuntime(),
      settings: { loadStore }
    });
  });

  afterEach(() => resetAppRuntimeForTests());

  it("consumes and resets the temporary welcome marker", async () => {
    await expect(consumeWelcomeDocumentState()).resolves.toBe(true);
    await expect(consumeWelcomeDocumentState()).resolves.toBe(false);
    await resetWelcomeDocumentState();
    await expect(consumeWelcomeDocumentState()).resolves.toBe(true);
  });
});
