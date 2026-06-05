import type { TerminalState } from "./instances";
import type { PaneDescriptor } from "./persistence";

export type PaneRestoreDecision =
  | {
      kind: "attach";
      ptyId: string;
      panePtyId: string;
      terminalState: TerminalState;
    }
  | { kind: "empty"; panePtyId: string; terminalState: TerminalState }
  | { kind: "strip" };

export function decidePaneRestore({
  descriptor,
  sessionId,
  livePtyIds,
}: {
  descriptor: PaneDescriptor;
  sessionId: string;
  livePtyIds: ReadonlySet<string> | null | undefined;
}): PaneRestoreDecision {
  if (descriptor.ptyId && livePtyIds?.has(descriptor.ptyId)) {
    return {
      kind: "attach",
      ptyId: descriptor.ptyId,
      panePtyId: descriptor.ptyId,
      terminalState: { kind: "attached", ptyId: descriptor.ptyId },
    };
  }

  if (descriptor.type === "command") return { kind: "strip" };

  return {
    kind: "empty",
    panePtyId: descriptor.ptyId === sessionId ? sessionId : "",
    terminalState: { kind: "empty" },
  };
}
