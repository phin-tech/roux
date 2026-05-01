/**
 * Repo quick-pick options derived from a flat list of paths. The `label` is
 * a short human-friendly display (last 2-3 path segments, expanded to disambiguate
 * collisions); `path` is the absolute repo path used for downstream calls.
 *
 * Extracted from NewSessionDialog so both the session picker and the new-project
 * dialog produce identical labels for the same repo set.
 */
export interface RepoQuickPickOption {
  label: string;
  path: string;
}

export function formatRepoShortLabel(path: string, depth: number = 2): string {
  const normalized = path.replaceAll("\\", "/");
  const segments = normalized.split("/").filter(Boolean);
  if (segments.length === 0) return path;
  if (segments.length === 1) return segments[0];
  return segments.slice(Math.max(segments.length - depth, 0)).join("/");
}

/**
 * Build display options. Starts with 2-segment labels; any label that collides
 * gets bumped to 3 segments, and if that still doesn't disambiguate the full
 * path is shown so the picker never displays two visually identical rows.
 */
export function buildQuickPickOptions(paths: string[]): RepoQuickPickOption[] {
  const firstPass = paths.map((path) => ({ path, label: formatRepoShortLabel(path, 2) }));
  const counts = new Map<string, number>();
  for (const item of firstPass) {
    counts.set(item.label, (counts.get(item.label) ?? 0) + 1);
  }
  return firstPass.map((item) => {
    if ((counts.get(item.label) ?? 0) === 1) return item;
    const deeper = formatRepoShortLabel(item.path, 3);
    return deeper === item.label ? { ...item, label: item.path } : { ...item, label: deeper };
  });
}

/**
 * Best-effort match for the user's typed input. Tries exact path, exact label
 * (case-insensitive), then a substring match in either field. Returns `null`
 * when the input is empty or nothing matches — callers typically treat that
 * as "commit the typed path as-is".
 */
export function findQuickPickMatch(
  queryRaw: string,
  options: RepoQuickPickOption[],
): RepoQuickPickOption | null {
  const query = queryRaw.trim();
  if (!query) return null;
  const lower = query.toLowerCase();
  const exactPath = options.find((o) => o.path === query);
  if (exactPath) return exactPath;
  const exactLabel = options.find((o) => o.label.toLowerCase() === lower);
  if (exactLabel) return exactLabel;
  return (
    options.find(
      (o) => o.label.toLowerCase().includes(lower) || o.path.toLowerCase().includes(lower),
    ) ?? null
  );
}
