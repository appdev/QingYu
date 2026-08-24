import type {MarkdownHostAdapter} from "./markra-core/adapter";

export type MarkdownAdapterOverrides = Partial<Pick<MarkdownHostAdapter,
    "openLink" | "resolveImageSource" | "saveClipboardAssets">>;

export const applyMarkdownAdapterOverrides = <T extends Pick<MarkdownHostAdapter,
    "openLink" | "resolveImageSource" | "saveClipboardAssets">>(
    adapter: T,
    overrides: MarkdownAdapterOverrides,
) => Object.assign(adapter, overrides);
