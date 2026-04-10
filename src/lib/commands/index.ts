import { registerPaneCommands } from "./panes";
import { registerSessionCommands } from "./sessions";
import { registerTaskCommands } from "./tasks";
import { registerWatchCommands } from "./watches";
import { registerUiCommands } from "./ui";

export function registerCommands() {
  registerPaneCommands();
  registerSessionCommands();
  registerTaskCommands();
  registerWatchCommands();
  registerUiCommands();
}

export { registry } from "./registry";
