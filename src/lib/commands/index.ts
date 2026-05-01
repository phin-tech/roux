import { registerPaneCommands } from "./panes";
import { registerSessionCommands } from "./sessions";
import { registerTaskCommands } from "./tasks";
import { registerWatchCommands } from "./watches";
import { registerUiCommands } from "./ui";
import { registerLibraryCommands } from "./library";
import { registerProjectCommands } from "./projects";

export function registerCommands() {
  registerPaneCommands();
  registerSessionCommands();
  registerTaskCommands();
  registerWatchCommands();
  registerUiCommands();
  registerLibraryCommands();
  registerProjectCommands();
}

export { registry } from "./registry";
