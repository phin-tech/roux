export function formatWorkItemStartError(error: unknown): string {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "Unknown error";

  if (message.includes("repoPath required") && message.includes("no project")) {
    return "Assign a project before starting this card.";
  }
  if (message.includes("requires a running daemon")) {
    return "Start requires a running daemon.";
  }
  if (message.includes("project has no repo_roots")) {
    return "The assigned project does not have a repository root.";
  }
  if (message.includes("project not found")) {
    return "The assigned project no longer exists.";
  }

  return `Start failed: ${message}`;
}
