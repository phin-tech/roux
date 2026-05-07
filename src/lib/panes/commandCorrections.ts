export interface CommandCorrection {
  id: string;
  label: string;
  replacement: string;
  description: string;
}

const GIT_SUBCOMMAND_CORRECTIONS: Record<string, string> = {
  brnach: "branch",
  chekout: "checkout",
  comit: "commit",
  pul: "pull",
  pus: "push",
  statsu: "status",
  stauts: "status",
};

const NPM_SCRIPT_SHORTHANDS = new Set([
  "build",
  "check",
  "dev",
  "format",
  "lint",
  "preview",
  "typecheck",
]);

function withFirstLineReplacement(text: string, replacementLine: string): string {
  const newlineIndex = text.indexOf("\n");
  if (newlineIndex === -1) return replacementLine;
  return replacementLine + text.slice(newlineIndex);
}

function firstLine(text: string): string {
  const newlineIndex = text.indexOf("\n");
  return newlineIndex === -1 ? text : text.slice(0, newlineIndex);
}

export function suggestCommandCorrection(text: string): CommandCorrection | null {
  const line = firstLine(text);

  const gtiMatch = line.match(/^([ \t]*)gti([ \t].*|)$/);
  if (gtiMatch) {
    const replacementLine = `${gtiMatch[1]}git${gtiMatch[2]}`;
    return {
      id: "gti",
      label: "Use git",
      replacement: withFirstLineReplacement(text, replacementLine),
      description: "Correct gti to git",
    };
  }

  const gitMatch = line.match(/^([ \t]*git[ \t]+)([A-Za-z-]+)(.*)$/);
  if (gitMatch) {
    const correctedSubcommand = GIT_SUBCOMMAND_CORRECTIONS[gitMatch[2]];
    if (correctedSubcommand) {
      const replacementLine = `${gitMatch[1]}${correctedSubcommand}${gitMatch[3]}`;
      return {
        id: `git-${gitMatch[2]}`,
        label: `Use git ${correctedSubcommand}`,
        replacement: withFirstLineReplacement(text, replacementLine),
        description: `Correct git ${gitMatch[2]} to git ${correctedSubcommand}`,
      };
    }
  }

  const npmMatch = line.match(/^([ \t]*npm[ \t]+)([A-Za-z][\w:-]*)(.*)$/);
  if (npmMatch && NPM_SCRIPT_SHORTHANDS.has(npmMatch[2])) {
    const replacementLine = `${npmMatch[1]}run ${npmMatch[2]}${npmMatch[3]}`;
    return {
      id: `npm-run-${npmMatch[2]}`,
      label: `Use npm run ${npmMatch[2]}`,
      replacement: withFirstLineReplacement(text, replacementLine),
      description: `Run the ${npmMatch[2]} package script`,
    };
  }

  return null;
}
