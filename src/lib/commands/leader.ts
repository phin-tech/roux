export interface LeaderHint {
  key: string;
  label: string;
}

type LeaderAction =
  | { kind: "command"; commandId: string }
  | { kind: "palette" };

interface LeaderNode {
  title: string;
  hints: LeaderHint[];
  pruneUnavailableHints?: boolean;
  children?: Record<string, LeaderNode | LeaderAction>;
}

export type LeaderResolution =
  | { kind: "pending"; title: string; hints: LeaderHint[] }
  | { kind: "command"; commandId: string }
  | { kind: "palette" }
  | { kind: "invalid" };

const PANE_NODE: LeaderNode = {
  title: "Panes",
  pruneUnavailableHints: true,
  hints: [
    { key: "h", label: "left" },
    { key: "j", label: "down" },
    { key: "k", label: "up" },
    { key: "l", label: "right" },
    { key: "s", label: "split" },
    { key: "v", label: "vsplit" },
    { key: "r", label: "rename" },
    { key: "d", label: "close" },
    { key: "f", label: "full" },
    { key: "t", label: "stack" },
  ],
  children: {
    h: { kind: "command", commandId: "pane.focus-left" },
    j: { kind: "command", commandId: "pane.focus-down" },
    k: { kind: "command", commandId: "pane.focus-up" },
    l: { kind: "command", commandId: "pane.focus-right" },
    s: { kind: "command", commandId: "pane.split-horizontal" },
    v: { kind: "command", commandId: "pane.split-vertical" },
    r: { kind: "command", commandId: "pane.rename" },
    d: { kind: "command", commandId: "pane.close" },
    f: { kind: "command", commandId: "pane.toggle-fullscreen" },
    t: { kind: "command", commandId: "pane.toggle-stack" },
  },
};

const SESSION_NODE: LeaderNode = {
  title: "Sessions",
  hints: [
    { key: "n", label: "new" },
    { key: "d", label: "close" },
    { key: "r", label: "reconnect" },
    { key: "e", label: "editor" },
  ],
  children: {
    n: { kind: "command", commandId: "session.new" },
    d: { kind: "command", commandId: "session.close" },
    r: { kind: "command", commandId: "session.reconnect" },
    e: { kind: "command", commandId: "session.open-in-editor" },
  },
};

const ROOT_NODE: LeaderNode = {
  title: "Leader",
  hints: [
    { key: "w", label: "panes" },
    { key: "b", label: "sessions" },
    { key: "n", label: "notes" },
    { key: "i", label: "inbox" },
    { key: "t", label: "watches" },
    { key: ",", label: "settings" },
    { key: "SPC", label: "commands" },
  ],
  children: {
    w: PANE_NODE,
    b: SESSION_NODE,
    n: { kind: "command", commandId: "ui.toggle-notes" },
    i: { kind: "command", commandId: "ui.toggle-notifications" },
    t: { kind: "command", commandId: "ui.toggle-watches" },
    ",": { kind: "command", commandId: "app.settings" },
    space: { kind: "palette" },
  },
};

function isAction(node: LeaderNode | LeaderAction): node is LeaderAction {
  return "kind" in node;
}

function hintKeyToChildKey(key: string): string {
  return key === "SPC" ? "space" : key;
}

function resolveLeaderNode(sequence: string[]): LeaderNode | LeaderAction | null {
  let current: LeaderNode | LeaderAction = ROOT_NODE;

  for (const key of sequence) {
    if (isAction(current)) return null;
    const next: LeaderNode | LeaderAction | undefined = current.children?.[key];
    if (!next) return null;
    current = next;
  }

  return current;
}

export function resolveLeaderSequence(sequence: string[]): LeaderResolution {
  const current = resolveLeaderNode(sequence);
  if (!current) return { kind: "invalid" };

  if (isAction(current)) {
    return current.kind === "palette"
      ? { kind: "palette" }
      : { kind: "command", commandId: current.commandId };
  }

  return {
    kind: "pending",
    title: current.title,
    hints: current.hints,
  };
}

export function getVisibleLeaderHints(
  sequence: string[],
  isCommandAvailable: (commandId: string) => boolean,
): LeaderHint[] {
  const current = resolveLeaderNode(sequence);
  if (!current || isAction(current)) return [];
  if (!current.pruneUnavailableHints) return current.hints;

  return current.hints.filter((hint) => {
    const child = current.children?.[hintKeyToChildKey(hint.key)];
    if (!child) return false;
    if (!isAction(child)) return true;
    if (child.kind === "palette") return true;
    return isCommandAvailable(child.commandId);
  });
}

export function normalizeLeaderKey(event: KeyboardEvent): string | null {
  if (event.metaKey || event.ctrlKey || event.altKey) return null;
  if (event.key === " ") return "space";
  if (event.key.length === 1) return event.key.toLowerCase();
  return null;
}
