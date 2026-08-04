import { useState } from "react";
import type { I18nKey } from "@markra/shared";
import { Button } from "@markra/ui";
import {
  formatMcpClientConnection,
  type McpClientConfigFormat
} from "../../lib/mcp-client-config";
import type { McpClientConnection } from "../../lib/mcp";
import { SettingsSection, SettingsSelect } from "./SettingsControls";

type CopyState = "configuration" | "instructions" | "error" | null;

async function writeSystemClipboard(text: string) {
  const clipboard = globalThis.navigator?.clipboard;
  if (!clipboard?.writeText) throw new Error("clipboard-unavailable");
  return clipboard.writeText(text);
}

export function McpClientConfiguration({
  connections,
  translate,
  writeClipboard = writeSystemClipboard
}: {
  connections: McpClientConnection[];
  translate: (key: I18nKey) => string;
  writeClipboard?: (text: string) => Promise<unknown>;
}) {
  const [format, setFormat] = useState<McpClientConfigFormat>("codex");
  const [copyState, setCopyState] = useState<CopyState>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const active = connections[selectedIndex] ?? null;
  const configuration = active ? formatMcpClientConnection(active, format) : "";

  const copy = async (kind: "configuration" | "instructions") => {
    const content = kind === "instructions"
      ? `${translate("settings.mcp.client.aiInstruction")}\n\n${configuration}`
      : configuration;
    try {
      await writeClipboard(content);
      setCopyState(kind);
    } catch {
      setCopyState("error");
    }
  };

  if (connections.length === 0) return null;

  return (
    <SettingsSection
      label={translate("settings.mcp.section.clientConnection")}
      intro={(
        <p className="m-0 max-w-[72ch] text-[12px] leading-5 text-(--text-secondary)">
          {translate("settings.mcp.client.summary")}
        </p>
      )}
    >
      <div className="mb-4 space-y-2">
        {connections.map((connection, index) => (
          <div
            key={connection.transport === "stdio" ? connection.command : connection.url}
            className={`overflow-hidden rounded-lg border transition-colors ${
              index === selectedIndex
                ? "border-(--accent) bg-(--bg-accent)"
                : "border-(--border-default) bg-(--bg-secondary)"
            }`}
          >
            <button
              className="flex w-full items-center gap-3 px-4 py-3 text-left"
              onClick={() => setSelectedIndex(index)}
            >
              <ConnectionTransportBadge transport={connection.transport} />
              <div className="min-w-0 flex-1">
                <code className="block truncate text-[12px] leading-5 text-(--text-heading)">
                  {connection.transport === "stdio" ? connection.command : connection.url}
                </code>
                {connection.transport === "http" && connection.tokenConfigured ? (
                  <p className="m-0 mt-1 text-[11px] leading-4 text-(--text-secondary)">
                    {translate("settings.mcp.client.bearerTokenConfigured")}
                  </p>
                ) : null}
              </div>
            </button>
          </div>
        ))}
      </div>
      {active ? (
        <div className="overflow-hidden rounded-lg border border-(--border-default) bg-(--bg-secondary)">
          <div className="grid grid-cols-2 divide-x divide-(--border-default) max-[640px]:grid-cols-1 max-[640px]:divide-x-0 max-[640px]:divide-y">
            <ConnectionFact
              label={translate("settings.mcp.client.transport")}
              value={translate(active.transport === "stdio"
                ? "settings.mcp.client.transportValue"
                : "settings.mcp.client.transportHttpValue")}
            />
            <ConnectionFact
              label={translate("settings.mcp.client.authentication")}
              value={active.transport === "http" && active.tokenConfigured
                ? translate("settings.mcp.client.bearerTokenConfigured")
                : translate("settings.mcp.client.authenticationValue")}
            />
          </div>
          <div className="border-t border-(--border-default) p-4">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
              <span className="text-[12px] leading-5 font-[650] text-(--text-heading)">
                {translate("settings.mcp.client.format")}
              </span>
              <SettingsSelect
                label={translate("settings.mcp.client.format")}
                value={format}
                options={[
                  { label: translate("settings.mcp.client.format.codex"), value: "codex" },
                  { label: translate("settings.mcp.client.format.json"), value: "json" }
                ]}
                onChange={(value) => setFormat(value === "json" ? "json" : "codex")}
              />
            </div>
            <pre className="m-0 max-h-64 overflow-auto rounded-md border border-(--border-default) bg-(--bg-primary) p-4 text-[12px] leading-5 text-(--text-heading)">
              <code>{configuration}</code>
            </pre>
            <div className="mt-3 flex flex-wrap items-center justify-end gap-2">
              <Button className="whitespace-nowrap" size="sm" onClick={() => {
                copy("configuration").catch(() => setCopyState("error"));
              }}>
                {translate("settings.mcp.client.copy")}
              </Button>
              <Button className="whitespace-nowrap" size="sm" onClick={() => {
                copy("instructions").catch(() => setCopyState("error"));
              }}>
                {translate("settings.mcp.client.copyForAi")}
              </Button>
            </div>
            {copyState ? (
              <p
                className={`m-0 mt-2 text-right text-[12px] leading-5 ${copyState === "error" ? "text-(--danger)" : "text-(--text-secondary)"}`}
                role={copyState === "error" ? "alert" : "status"}
              >
                {translate(copyStateMessage(copyState))}
              </p>
            ) : null}
          </div>
        </div>
      ) : null}
    </SettingsSection>
  );
}

function ConnectionTransportBadge({ transport }: { transport: McpClientConnection["transport"] }) {
  return (
    <span className="inline-flex shrink-0 items-center rounded-md border border-(--border-default) bg-(--bg-primary) px-2 py-0.5 text-[11px] leading-4 font-[650] text-(--text-heading) uppercase">
      {transport}
    </span>
  );
}

function ConnectionFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 px-4 py-3">
      <p className="m-0 text-[11px] leading-4 font-[560] text-(--text-secondary)">{label}</p>
      <p className="m-0 mt-1 break-words text-[13px] leading-5 font-[650] text-(--text-heading)">{value}</p>
    </div>
  );
}

function copyStateMessage(state: Exclude<CopyState, null>): I18nKey {
  if (state === "configuration") return "settings.mcp.client.copied";
  if (state === "instructions") return "settings.mcp.client.instructionsCopied";
  return "settings.mcp.client.copyError";
}
