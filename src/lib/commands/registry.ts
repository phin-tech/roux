export interface CommandItem {
  id: string;
  label: string;
  description?: string;
  icon?: string;
  disabledReason?: string | null;
  action?: () => void | Promise<void>;
  /** If set, selecting this item drills into a sub-step instead of executing */
  substeps?: () => CommandItem[] | Promise<CommandItem[]>;
  /** If set, selecting this item drills into the specified command (for onInput flow) */
  drillCommand?: string;
}

export interface Command {
  id: string;
  label: string;
  category: string;
  /** Whether this command is available in current context */
  available?: () => boolean;
  /** If set, command remains visible but cannot be executed. */
  disabledReason?: () => string | null;
  /** For simple commands: just execute */
  execute?: () => void | Promise<void>;
  /**
   * For multi-step commands: return items for the first step.
   * When an item has substeps, the palette drills in.
   * When an item has action, it executes and closes.
   */
  getItems?: () => CommandItem[] | Promise<CommandItem[]>;
  /** Placeholder text shown in the input when drilled into this command */
  inputPlaceholder?: string;
  /** Called when user presses Enter with text that doesn't match any item */
  onInput?: (text: string) => void | Promise<void>;
}

export class CommandRegistry {
  private commands: Map<string, Command> = new Map();

  register(command: Command) {
    this.commands.set(command.id, command);
  }

  unregister(id: string) {
    this.commands.delete(id);
  }

  getAvailable(): Command[] {
    return [...this.commands.values()].filter(
      (c) => !c.available || c.available(),
    );
  }

  get(id: string): Command | undefined {
    return this.commands.get(id);
  }

  execute(id: string) {
    const cmd = this.commands.get(id);
    if (cmd?.execute) cmd.execute();
  }
}

export const registry = new CommandRegistry();
