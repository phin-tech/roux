import { registerPaneCommands } from "./panes";
import { registerSessionCommands } from "./sessions";
import { registerTaskCommands } from "./tasks";
import { registerWatchCommands } from "./watches";
import { registerUiCommands } from "./ui";
import { registerLibraryCommands } from "./library";
import { registerProjectCommands } from "./projects";
import { registerExternalToolCommands } from "./externalTools";

export function registerCommands() {
  registerPaneCommands();
  registerSessionCommands();
  registerTaskCommands();
  registerWatchCommands();
  registerUiCommands();
  registerLibraryCommands();
  registerProjectCommands();
  registerExternalToolCommands();
}

export { registry } from "./registry";
