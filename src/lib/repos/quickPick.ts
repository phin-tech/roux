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
 * gets bumped to 3 segments, and any label that *still* collides at 3 segments
 * (or where 3 segments equals the original path) falls back to the full path —
 * the picker never displays two visually identical rows.
 */
export function buildQuickPickOptions(paths: string[]): RepoQuickPickOption[] {
  const firstPass = paths.map((path) => ({
    path,
    label: formatRepoShortLabel(path, 2),
  }));
  const firstCounts = new Map<string, number>();
  for (const item of firstPass) {
    firstCounts.set(item.label, (firstCounts.get(item.label) ?? 0) + 1);
  }
  const secondPass = firstPass.map((item) => {
    if ((firstCounts.get(item.label) ?? 0) === 1) return item;
    return { ...item, label: formatRepoShortLabel(item.path, 3) };
  });
  // Recount after the bump: two paths may share the same 3-segment tail
  // (`/a/x/y/repo` vs `/b/x/y/repo`), in which case we have to show the
  // full path to honor the doc-comment promise.
  const secondCounts = new Map<string, number>();
  for (const item of secondPass) {
    secondCounts.set(item.label, (secondCounts.get(item.label) ?? 0) + 1);
  }
  return secondPass.map((item) =>
    (secondCounts.get(item.label) ?? 0) === 1
      ? item
      : { ...item, label: item.path },
  );
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
      (o) =>
        o.label.toLowerCase().includes(lower) ||
        o.path.toLowerCase().includes(lower),
    ) ?? null
  );
}
