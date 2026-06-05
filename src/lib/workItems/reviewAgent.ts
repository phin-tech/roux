interface ResolveReviewAgentRepoRootInput {
  itemRepoPath: string | null;
  projectRepoRoots: string[];
  worktreePath: string;
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

function rootOwnsWorktree(root: string, worktreePath: string): boolean {
  const normalizedRoot = normalizePath(root);
  const normalizedWorktree = normalizePath(worktreePath);
  return (
    normalizedWorktree === normalizedRoot ||
    normalizedWorktree.startsWith(`${normalizedRoot}/`) ||
    normalizedWorktree.startsWith(`${normalizedRoot}.roux-card`) ||
    normalizedWorktree.startsWith(`${normalizedRoot}-roux-card`)
  );
}

export function resolveReviewAgentRepoRoot({
  itemRepoPath,
  projectRepoRoots,
  worktreePath,
}: ResolveReviewAgentRepoRootInput): string | null {
  if (itemRepoPath) return itemRepoPath;
  return (
    [...projectRepoRoots]
      .sort(
        (left, right) =>
          normalizePath(right).length - normalizePath(left).length,
      )
      .find((root) => rootOwnsWorktree(root, worktreePath)) ?? null
  );
}
