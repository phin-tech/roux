import type { GroupBy, Project, Session } from "$lib/types";

export interface SessionGroup {
  name: string;
  key: string;
  sessions: Session[];
  latest: number;
}

const UNTAGGED_KEY = "__untagged__";

function groupByRepo(sessions: Session[]): SessionGroup[] {
  const map = new Map<string, SessionGroup>();
  for (const s of sessions) {
    let group = map.get(s.repoRoot);
    if (!group) {
      group = {
        name: s.repoRoot.split("/").pop() || s.repoRoot,
        key: s.repoRoot,
        sessions: [],
        latest: 0,
      };
      map.set(s.repoRoot, group);
    }
    group.sessions.push(s);
    if (s.createdAt > group.latest) group.latest = s.createdAt;
  }
  return [...map.values()].sort((a, b) => b.latest - a.latest);
}

function groupByProject(
  sessions: Session[],
  projects: Project[],
): SessionGroup[] {
  const map = new Map<string, SessionGroup>();
  for (const s of sessions) {
    const key = s.projectId ?? UNTAGGED_KEY;
    let group = map.get(key);
    if (!group) {
      const project = projects.find((p) => p.id === s.projectId);
      group = {
        name: project?.name ?? "Untagged",
        key,
        sessions: [],
        latest: 0,
      };
      map.set(key, group);
    }
    group.sessions.push(s);
    if (s.createdAt > group.latest) group.latest = s.createdAt;
  }
  // Seed empty groups for projects that have blueprints but no live sessions
  // yet — otherwise the sidebar's "spawn from sidebar" affordance and the
  // edit handle on the group header are unreachable until the user creates
  // a session some other way. `latest = 0` keeps these groups below any
  // group that has activity, just above the Untagged tail.
  for (const p of projects) {
    if (map.has(p.id)) continue;
    if (!p.sessionBlueprints || p.sessionBlueprints.length === 0) continue;
    map.set(p.id, { name: p.name, key: p.id, sessions: [], latest: 0 });
  }
  const groups = [...map.values()].sort((a, b) => b.latest - a.latest);
  const untaggedIdx = groups.findIndex((g) => g.key === UNTAGGED_KEY);
  if (untaggedIdx > 0) {
    const [untagged] = groups.splice(untaggedIdx, 1);
    groups.push(untagged);
  }
  return groups;
}

const ALL_KEY = "__all__";

function groupBySession(sessions: Session[]): SessionGroup[] {
  if (sessions.length === 0) return [];
  const sorted = [...sessions].sort((a, b) => b.createdAt - a.createdAt);
  const latest = sorted.reduce((m, s) => (s.createdAt > m ? s.createdAt : m), 0);
  return [{ name: "Sessions", key: ALL_KEY, sessions: sorted, latest }];
}

export function getGroupedSessions(
  sessions: Session[],
  projects: Project[],
  groupBy: GroupBy,
): SessionGroup[] {
  switch (groupBy) {
    case "project":
      return groupByProject(sessions, projects);
    case "session":
      return groupBySession(sessions);
    case "repo":
    default:
      return groupByRepo(sessions);
  }
}

export function getVisualSessionOrder(
  sessions: Session[],
  projects: Project[],
  groupBy: GroupBy,
): Session[] {
  const groups = getGroupedSessions(sessions, projects, groupBy);
  const flat: Session[] = [];
  for (const g of groups) flat.push(...g.sessions);
  return flat;
}
