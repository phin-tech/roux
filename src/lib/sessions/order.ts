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
  const groups = [...map.values()].sort((a, b) => b.latest - a.latest);
  const untaggedIdx = groups.findIndex((g) => g.key === UNTAGGED_KEY);
  if (untaggedIdx > 0) {
    const [untagged] = groups.splice(untaggedIdx, 1);
    groups.push(untagged);
  }
  return groups;
}

export function getGroupedSessions(
  sessions: Session[],
  projects: Project[],
  groupBy: GroupBy,
): SessionGroup[] {
  return groupBy === "project"
    ? groupByProject(sessions, projects)
    : groupByRepo(sessions);
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
