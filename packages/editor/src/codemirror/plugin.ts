import { Facet, type EditorState, type Extension } from "@codemirror/state";
import {
  keymap,
  type EditorView,
  type KeyBinding,
} from "@codemirror/view";

export type MarkraUiPlacement =
  | "toolbar"
  | "selection-toolbar"
  | "slash-menu"
  | "context-menu";

export type MarkraKeyBinding = Omit<KeyBinding, "any" | "run" | "shift">;

export interface MarkraCommand {
  readonly id: string;
  readonly label: string;
  readonly description?: string;
  readonly keybindings?: readonly MarkraKeyBinding[];
  readonly isActive?: (view: EditorView) => boolean;
  readonly isEnabled?: (view: EditorView) => boolean;
  readonly run: (view: EditorView) => boolean;
}

export interface MarkraUiContribution {
  readonly command: string;
  readonly placement: MarkraUiPlacement;
  readonly label?: string;
  readonly icon?: string;
  readonly group?: string;
  readonly keywords?: readonly string[];
  readonly order?: number;
  readonly when?: (view: EditorView) => boolean;
}

export interface MarkraPlugin {
  readonly id: string;
  readonly extension?: Extension;
  readonly commands?: readonly MarkraCommand[];
  readonly ui?: readonly MarkraUiContribution[];
}

export interface MarkraUiAction {
  readonly active: boolean;
  readonly command: string;
  readonly description?: string;
  readonly enabled: boolean;
  readonly group?: string;
  readonly icon?: string;
  readonly keywords: readonly string[];
  readonly label: string;
  readonly order: number;
  readonly placement: MarkraUiPlacement;
  readonly plugin: string;
  run: () => boolean;
}

interface RegisteredCommand {
  command: MarkraCommand;
  plugin: string;
}

interface MarkraRegistry {
  commands: ReadonlyMap<string, RegisteredCommand>;
  plugins: readonly MarkraPlugin[];
  ui: readonly MarkraUiContribution[];
}

function createRegistry(plugins: readonly MarkraPlugin[]): MarkraRegistry {
  const pluginIds = new Set<string>();
  const commands = new Map<string, RegisteredCommand>();
  const ui: MarkraUiContribution[] = [];

  for (const plugin of plugins) {
    if (pluginIds.has(plugin.id)) {
      throw new Error(`Duplicate Markra plugin id "${plugin.id}"`);
    }
    pluginIds.add(plugin.id);

    for (const command of plugin.commands ?? []) {
      if (commands.has(command.id)) {
        throw new Error(`Duplicate Markra command id "${command.id}"`);
      }
      commands.set(command.id, { command, plugin: plugin.id });
    }

    ui.push(...(plugin.ui ?? []));
  }

  for (const contribution of ui) {
    if (!commands.has(contribution.command)) {
      throw new Error(
        `Unknown Markra command id "${contribution.command}" in UI contribution`,
      );
    }
  }

  return { commands, plugins: [...plugins], ui };
}

const registryFacet = Facet.define<MarkraPlugin, MarkraRegistry>({
  combine: createRegistry,
});

export function defineMarkraPlugin(plugin: MarkraPlugin): MarkraPlugin {
  if (!plugin.id.trim()) {
    throw new Error("Markra plugin id must not be empty");
  }
  return plugin;
}

export function markraPlugins(
  plugins: readonly MarkraPlugin[],
): Extension {
  const bindings: KeyBinding[] = [];

  for (const plugin of plugins) {
    for (const command of plugin.commands ?? []) {
      for (const binding of command.keybindings ?? []) {
        bindings.push({
          ...binding,
          run: (view) => runMarkraCommand(view, command.id),
        });
      }
    }
  }

  return [
    plugins.map((plugin) => [
      registryFacet.of(plugin),
      plugin.extension ?? [],
    ]),
    bindings.length > 0 ? keymap.of(bindings) : [],
  ];
}

export function listMarkraPlugins(
  state: EditorState,
): readonly MarkraPlugin[] {
  return state.facet(registryFacet).plugins;
}

export function runMarkraCommand(view: EditorView, id: string) {
  const registered = view.state.facet(registryFacet).commands.get(id);
  if (!registered) return false;
  if (registered.command.isEnabled?.(view) === false) return false;
  return registered.command.run(view);
}

export function listMarkraUi(
  view: EditorView,
  placement: MarkraUiPlacement,
): MarkraUiAction[] {
  const registry = view.state.facet(registryFacet);

  return registry.ui
    .filter(
      (contribution) =>
        contribution.placement === placement && contribution.when?.(view) !== false,
    )
    .map((contribution) => {
      const registered = registry.commands.get(contribution.command);
      if (!registered) {
        throw new Error(`Unknown Markra command id "${contribution.command}"`);
      }

      const { command, plugin } = registered;
      return {
        active: command.isActive?.(view) ?? false,
        command: command.id,
        description: command.description,
        enabled: command.isEnabled?.(view) ?? true,
        group: contribution.group,
        icon: contribution.icon,
        keywords: contribution.keywords ?? [],
        label: contribution.label ?? command.label,
        order: contribution.order ?? 0,
        placement: contribution.placement,
        plugin,
        run: () => runMarkraCommand(view, command.id),
      };
    })
    .sort((left, right) => left.order - right.order || left.label.localeCompare(right.label));
}

function normalizedSearchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase().replace(/\s+/gu, "");
}

export function searchMarkraUi(
  view: EditorView,
  placement: MarkraUiPlacement,
  query: string,
) {
  const actions = listMarkraUi(view, placement);
  const normalizedQuery = normalizedSearchText(query);
  if (!normalizedQuery) return actions;

  return actions.filter((action) =>
    [
      action.command,
      action.description ?? "",
      action.group ?? "",
      action.label,
      action.plugin,
      ...action.keywords,
    ].some((candidate) =>
      normalizedSearchText(candidate).includes(normalizedQuery),
    ),
  );
}
