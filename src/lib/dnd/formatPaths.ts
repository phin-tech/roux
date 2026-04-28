const SAFE_CHARS = /^[A-Za-z0-9_\-./@%+=:,]+$/;

function shellQuote(value: string): string {
  if (value.length > 0 && SAFE_CHARS.test(value)) return value;
  return `'${value.replace(/'/g, "'\\''")}'`;
}

export function formatPathsForTerminal(paths: readonly string[]): string {
  return paths.map(shellQuote).join(" ");
}
