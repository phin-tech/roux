import { get } from "svelte/store";
import { sessionLayouts, type LayoutNode } from "./layout";
import { paneInstances, getAttachedPtyId } from "./instances";

export interface PaneDescriptorSnapshot {
  id: string;
  type: string;
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  profileId?: string;
}

export interface PaneTreeSnapshot {
  sessionId: string;
  layout: LayoutNode | null;
  descriptors: PaneDescriptorSnapshot[];
}

/**
 * Walk the pane tree for a session and return a serializable snapshot
 * combining the layout shape with the runtime metadata for each pane.
 *
 * Used by the CLI (`roux session panes list`) via a socket round-trip
 * so external callers can see the live pane topology without reading
 * stale on-disk state.
 */
export function collectPaneTree(sessionId: string): PaneTreeSnapshot {
  const layout = get(sessionLayouts).get(sessionId) ?? null;
  const instances = get(paneInstances);

  const descriptors: PaneDescriptorSnapshot[] = [];
  if (layout) {
    const visit = (node: LayoutNode) => {
      if (node.kind === "leaf") {
        const inst = instances.get(node.paneId);
        if (inst) {
          descriptors.push({
            id: inst.id,
            type: inst.type,
            ptyId: getAttachedPtyId(inst) ?? "",
            name: inst.name,
            workingDir: inst.workingDir,
            command: inst.command,
            profileId:
              inst.spawnProfileRef?.kind === "registered"
                ? inst.spawnProfileRef.id
                : inst.spawnProfileRef?.kind === "inline"
                  ? inst.spawnProfileRef.profile.id
                  : undefined,
          });
        } else {
          descriptors.push({ id: node.paneId, type: "unknown", ptyId: "" });
        }
      } else {
        for (const child of node.children) visit(child);
      }
    };
    visit(layout);
  }

  return { sessionId, layout, descriptors };
}
