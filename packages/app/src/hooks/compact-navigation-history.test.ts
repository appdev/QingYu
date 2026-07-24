import {
  compactHistoryState,
  compactStackFromHistoryState,
  createCompactNavigationSessionId
} from "./compact-navigation-history";
import type { CompactNavigationState } from "./useCompactNavigation";

describe("Compact navigation history markers", () => {
  const stack: CompactNavigationState = [
    { kind: "editor" },
    { kind: "sync-status" },
    { kind: "sync-form", mode: "edit" }
  ];

  it("preserves unrelated state and decodes only the owning session", () => {
    const state = compactHistoryState({ external: "kept" }, stack, "active-session");

    expect(state).toMatchObject({ external: "kept" });
    expect(compactStackFromHistoryState(state, "active-session")).toEqual(stack);
    expect(compactStackFromHistoryState(state, "stale-session")).toBeNull();
  });

  it("restores the Compact MCP settings destination", () => {
    const mcpStack: CompactNavigationState = [
      { kind: "editor" },
      { kind: "settings" },
      { kind: "settings-detail", category: "mcp" }
    ];

    const state = compactHistoryState({}, mcpStack, "mcp-session");

    expect(compactStackFromHistoryState(state, "mcp-session")).toEqual(mcpStack);
  });

  it("rejects malformed stacks instead of restoring them", () => {
    const malformedState = compactHistoryState({}, stack, "active-session") as Record<string, unknown>;
    malformedState.__markraCompactNavigation = {
      sessionId: "active-session",
      stack: [{ kind: "files" }]
    };

    expect(compactStackFromHistoryState(malformedState, "active-session")).toBeNull();
  });

  it("prefers UUIDs and makes the fallback unique with a sequence", () => {
    const uuid = "00000000-0000-4000-8000-000000000002";
    expect(createCompactNavigationSessionId({ randomUUID: () => uuid })).toBe(uuid);

    const sources = {
      now: () => 1234,
      random: () => 0.25,
      randomUUID: null
    };
    const firstFallback = createCompactNavigationSessionId(sources);
    const secondFallback = createCompactNavigationSessionId(sources);

    expect(firstFallback).toContain("ya");
    expect(firstFallback).not.toBe(secondFallback);
  });
});
