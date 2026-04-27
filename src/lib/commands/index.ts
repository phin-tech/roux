import { registerPaneCommands } from "./panes";
import { registerSessionCommands } from "./sessions";
import { registerTaskCommands } from "./tasks";
import { registerWatchCommands } from "./watches";
import { registerUiCommands } from "./ui";
import { registerLibraryCommands } from "./library";

export function registerCommands() {
  registerPaneCommands();
  registerSessionCommands();
  registerTaskCommands();
  registerWatchCommands();
  registerUiCommands();
  registerLibraryCommands();
}

export { registry } from "./registry";
