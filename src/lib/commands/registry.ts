export interface CommandItem {
  id: string;
  label: string;
  description?: string;
  icon?: string;
  action?: () => void | Promise<void>;
  /** If set, selecting this item drills into a sub-step instead of executing */
  substeps?: () => CommandItem[] | Promise<CommandItem[]>;
}

export interface Command {
  id: string;
  label: string;
  shortcut?: string;
  category: string;
  /** Whether this command is available in current context */
  available?: () => boolean;
  /** For simple commands: just execute */
  execute?: () => void | Promise<void>;
  /**
   * For multi-step commands: return items for the first step.
   * When an item has substeps, the palette drills in.
   * When an item has action, it executes and closes.
   */
  getItems?: () => CommandItem[] | Promise<CommandItem[]>;
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
      (c) => !c.available || c.available()
    );
  }

  getByShortcut(shortcut: string): Command | undefined {
    return [...this.commands.values()].find((c) => c.shortcut === shortcut);
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
