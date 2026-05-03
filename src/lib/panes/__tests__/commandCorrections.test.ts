import { describe, expect, it } from "vitest";

import { suggestCommandCorrection } from "../commandCorrections";

describe("suggestCommandCorrection", () => {
  it("corrects gti to git", () => {
    expect(suggestCommandCorrection("gti status")).toMatchObject({
      id: "gti",
      replacement: "git status",
    });
  });

  it("preserves leading whitespace", () => {
    expect(suggestCommandCorrection("  gti status")?.replacement).toBe("  git status");
  });

  it("preserves following lines when correcting the first command", () => {
    expect(suggestCommandCorrection("gti status\necho done")?.replacement).toBe("git status\necho done");
  });

  it("corrects common git subcommand typos", () => {
    expect(suggestCommandCorrection("git statsu --short")?.replacement).toBe("git status --short");
    expect(suggestCommandCorrection("git comit -m test")?.replacement).toBe("git commit -m test");
    expect(suggestCommandCorrection("git chekout main")?.replacement).toBe("git checkout main");
  });

  it("adds npm run for script-like npm invocations", () => {
    expect(suggestCommandCorrection("npm dev")?.replacement).toBe("npm run dev");
    expect(suggestCommandCorrection("npm build -- --watch")?.replacement).toBe("npm run build -- --watch");
  });

  it("does not rewrite valid npm lifecycle shortcuts", () => {
    expect(suggestCommandCorrection("npm test")).toBeNull();
    expect(suggestCommandCorrection("npm start")).toBeNull();
  });

  it("does not rewrite commands outside the first command token", () => {
    expect(suggestCommandCorrection("echo gti status")).toBeNull();
    expect(suggestCommandCorrection("git status")).toBeNull();
  });
});
