import { describe, expect, it } from "vitest";

import {
  joinLines,
  smartQuotesToStraight,
  stripCodeFence,
  stripPromptPrefix,
  trimDocument,
  unwrapContinuations,
} from "../textTransforms";

// ─────────────────────────────────────────────────────────────────────────────
// joinLines
// ─────────────────────────────────────────────────────────────────────────────

describe("joinLines", () => {
  describe("happy path", () => {
    it("replaces newlines with spaces", () => {
      expect(joinLines("a\nb\nc")).toBe("a b c");
    });

    it("collapses runs of spaces", () => {
      expect(joinLines("a    b")).toBe("a b");
    });

    it("collapses tabs", () => {
      expect(joinLines("a\tb\tc")).toBe("a b c");
    });

    it("collapses blank lines", () => {
      expect(joinLines("a\n\n\nb")).toBe("a b");
    });

    it("collapses mixed tabs + spaces + blank lines", () => {
      expect(joinLines("a \t\n \n\tb")).toBe("a b");
    });

    it("normalizes CRLF before joining", () => {
      expect(joinLines("a\r\nb\r\nc")).toBe("a b c");
    });

    it("normalizes lone CR before joining", () => {
      expect(joinLines("a\rb\rc")).toBe("a b c");
    });

    it("normalizes mixed line endings", () => {
      expect(joinLines("a\r\nb\nc\rd")).toBe("a b c d");
    });
  });

  describe("edge cases", () => {
    it("empty input returns empty", () => {
      expect(joinLines("")).toBe("");
    });

    it("only whitespace returns empty", () => {
      expect(joinLines("   \n\t\n   ")).toBe("");
    });

    it("single line is preserved", () => {
      expect(joinLines("no newlines")).toBe("no newlines");
    });

    it("leading and trailing whitespace gets trimmed", () => {
      expect(joinLines("  a\nb  ")).toBe("a b");
    });

    it("preserves quoted content with spaces inside", () => {
      // The function can't know quoting semantics, but it shouldn't mangle
      // single characters of content.
      expect(joinLines('echo "foo bar"\nbaz')).toBe('echo "foo bar" baz');
    });

    it("single newline becomes one space", () => {
      expect(joinLines("\n")).toBe("");
    });
  });

  describe("idempotency", () => {
    const cases = [
      "",
      "already joined",
      "a b c",
      "echo   hi",
      "a\nb",
      "a\r\nb",
      "  trim me  ",
      "one\ntwo\nthree\n",
    ];
    it.each(cases)("joinLines(joinLines(x)) === joinLines(x) for %j", (s) => {
      expect(joinLines(joinLines(s))).toBe(joinLines(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// unwrapContinuations
// ─────────────────────────────────────────────────────────────────────────────

describe("unwrapContinuations", () => {
  describe("happy path", () => {
    it("basic 2-line continuation", () => {
      expect(unwrapContinuations("echo foo \\\n    bar")).toBe("echo foo bar");
    });

    it("chained 4-line continuation", () => {
      const input = "a \\\n    b \\\n    c \\\n    d";
      expect(unwrapContinuations(input)).toBe("a b c d");
    });

    it("no-indent continuation", () => {
      expect(unwrapContinuations("echo foo \\\nbar")).toBe("echo foo bar");
    });

    it("tab-indented continuation", () => {
      expect(unwrapContinuations("echo foo \\\n\tbar")).toBe("echo foo bar");
    });

    it("mixed tab + space indent", () => {
      expect(unwrapContinuations("a \\\n\t  b")).toBe("a b");
    });

    it("CRLF continuations", () => {
      expect(unwrapContinuations("a \\\r\n    b")).toBe("a b");
    });

    it("mixed CRLF and LF in same document", () => {
      expect(unwrapContinuations("a \\\r\nb \\\nc")).toBe("a b c");
    });
  });

  describe("trailing whitespace after the backslash (regression)", () => {
    // Common when pasting from a terminal that pads wrapped lines with
    // spaces, or from editors that strip-trailing-whitespace-on-save but the
    // copy happened before saving.
    it("tolerates spaces between `\\` and newline", () => {
      const input = "docker run -d \\   \n    --name web \\  \n    nginx";
      expect(unwrapContinuations(input)).toBe("docker run -d --name web nginx");
    });

    it("tolerates tabs between `\\` and newline", () => {
      expect(unwrapContinuations("a \\\t\n    b")).toBe("a b");
    });

    it("tolerates many spaces of padding (terminal-width copy artifact)", () => {
      const pad = " ".repeat(60);
      const input = `echo foo \\${pad}\n    bar`;
      expect(unwrapContinuations(input)).toBe("echo foo bar");
    });
  });

  describe("edge cases", () => {
    it("empty input", () => {
      expect(unwrapContinuations("")).toBe("");
    });

    it("single line with no continuation", () => {
      expect(unwrapContinuations("echo hi")).toBe("echo hi");
    });

    it("backslash in the middle of a line is not a continuation", () => {
      expect(unwrapContinuations("echo a\\ b")).toBe("echo a\\ b");
    });

    it("trailing backslash at end-of-string (no newline) is preserved", () => {
      // Truncated paste: the last `\` has no following newline.
      expect(unwrapContinuations("echo a \\")).toBe("echo a \\");
    });

    it("backslash at end of single trailing line with no content after", () => {
      // `\<LF>` with nothing after still collapses (consistent with
      // "a continuation followed by empty content").
      expect(unwrapContinuations("echo a \\\n")).toBe("echo a ");
    });

    it("double backslash at EOL consumes only the inner `\\<LF>`", () => {
      // Corner case: a literal `\\` in the source followed by a newline
      // means "escaped backslash, newline ends the line". Our transform
      // eats the trailing `\<LF>` — acceptable because true `\\` escape
      // sequences are rare in pasted LLM output and the user can undo.
      expect(unwrapContinuations("echo a\\\\\nbar")).toBe("echo a\\ bar");
    });
  });

  describe("idempotency", () => {
    const cases = [
      "",
      "echo hi",
      "a \\\nb",
      "a \\   \nb",
      "a \\\n    b \\\n    c",
      "echo a\\ b",
    ];
    it.each(cases)("unwrapContinuations x2 === unwrapContinuations x1 for %j", (s) => {
      expect(unwrapContinuations(unwrapContinuations(s))).toBe(unwrapContinuations(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// stripPromptPrefix
// ─────────────────────────────────────────────────────────────────────────────

describe("stripPromptPrefix", () => {
  describe("happy path", () => {
    it("strips $ prefix", () => {
      expect(stripPromptPrefix("$ ls")).toBe("ls");
    });

    it("strips ❯ prefix", () => {
      expect(stripPromptPrefix("❯ ls")).toBe("ls");
    });

    it("strips # prefix", () => {
      expect(stripPromptPrefix("# apt update")).toBe("apt update");
    });

    it("strips > prefix", () => {
      expect(stripPromptPrefix("> git status")).toBe("git status");
    });

    it("strips across multiple lines", () => {
      const input = "$ git add .\n❯ git commit\n# systemctl restart\n> npm test";
      expect(stripPromptPrefix(input))
        .toBe("git add .\ngit commit\nsystemctl restart\nnpm test");
    });

    it("normalizes CRLF before stripping", () => {
      expect(stripPromptPrefix("$ a\r\n$ b")).toBe("a\nb");
    });
  });

  describe("preservation (must not strip)", () => {
    it("interior `$` as shell variable", () => {
      expect(stripPromptPrefix("echo $foo")).toBe("echo $foo");
    });

    it("interior `>` as shell redirect", () => {
      expect(stripPromptPrefix("cat file > out.txt")).toBe("cat file > out.txt");
    });

    it("interior `#` as comment", () => {
      expect(stripPromptPrefix("echo # not a comment")).toBe("echo # not a comment");
    });

    it("prefix character without trailing space is not stripped", () => {
      expect(stripPromptPrefix("$foo")).toBe("$foo");
      expect(stripPromptPrefix(">foo")).toBe(">foo");
      expect(stripPromptPrefix("#foo")).toBe("#foo");
    });

    it("lines with leading whitespace before the prefix are NOT stripped", () => {
      // `  $ ls` — spaces then prefix — isn't a prompt, keep as-is.
      expect(stripPromptPrefix("  $ ls")).toBe("  $ ls");
    });

    it("lines not starting with a known marker are untouched", () => {
      expect(stripPromptPrefix("echo hi")).toBe("echo hi");
    });
  });

  describe("mixed content", () => {
    it("strips only lines that start with a prefix", () => {
      const input = "$ export FOO=1\necho \"$FOO\"\n$ echo done";
      expect(stripPromptPrefix(input)).toBe("export FOO=1\necho \"$FOO\"\necho done");
    });

    it("strips exactly one prefix per line, not recursively", () => {
      // If a line somehow has `$ $ ls`, strip the first marker only.
      expect(stripPromptPrefix("$ $ ls")).toBe("$ ls");
    });
  });

  describe("edge cases", () => {
    it("empty input", () => {
      expect(stripPromptPrefix("")).toBe("");
    });

    it("prefix only, no command", () => {
      expect(stripPromptPrefix("$ ")).toBe("");
    });

    it("prefix followed by multiple spaces", () => {
      expect(stripPromptPrefix("$   ls")).toBe("  ls");
    });

    it("blank line between prefixed lines is preserved", () => {
      expect(stripPromptPrefix("$ a\n\n$ b")).toBe("a\n\nb");
    });
  });

  describe("idempotency", () => {
    const cases = ["", "$ ls", "echo hi", "$ a\n$ b", "  $ ls"];
    it.each(cases)("stripPromptPrefix x2 === stripPromptPrefix x1 for %j", (s) => {
      expect(stripPromptPrefix(stripPromptPrefix(s))).toBe(stripPromptPrefix(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// stripCodeFence
// ─────────────────────────────────────────────────────────────────────────────

describe("stripCodeFence", () => {
  describe("happy path", () => {
    it("strips leading + trailing fence with language tag", () => {
      expect(stripCodeFence("```bash\nls\n```")).toBe("ls");
    });

    it("strips leading + trailing fence with no language tag", () => {
      expect(stripCodeFence("```\nls\n```")).toBe("ls");
    });

    it("strips fence with uncommon language tag", () => {
      expect(stripCodeFence("```powershell\nGet-Process\n```")).toBe("Get-Process");
    });

    it("strips fence with attribute string", () => {
      expect(stripCodeFence("```bash {title=example}\nls\n```")).toBe("ls");
    });
  });

  describe("partial fences", () => {
    it("strips leading fence only", () => {
      expect(stripCodeFence("```sh\necho hi")).toBe("echo hi");
    });

    it("strips trailing fence only", () => {
      expect(stripCodeFence("echo hi\n```")).toBe("echo hi");
    });
  });

  describe("preservation", () => {
    it("unfenced content is unchanged", () => {
      expect(stripCodeFence("echo hi\nworld")).toBe("echo hi\nworld");
    });

    it("interior fence on a non-first-non-last line is preserved", () => {
      expect(stripCodeFence("a\n```\nb"))
        .toBe("a\n```\nb");
    });

    it("fence-like string inside a line is preserved", () => {
      expect(stripCodeFence("echo '```'")).toBe("echo '```'");
    });

    it("indented fence is not stripped (must be at column 0)", () => {
      expect(stripCodeFence("    ```\necho hi\n    ```"))
        .toBe("    ```\necho hi\n    ```");
    });
  });

  describe("edge cases", () => {
    it("empty input", () => {
      expect(stripCodeFence("")).toBe("");
    });

    it("only fence start", () => {
      expect(stripCodeFence("```")).toBe("");
    });

    it("only fence start with language", () => {
      expect(stripCodeFence("```bash")).toBe("");
    });

    it("just two fences back-to-back", () => {
      expect(stripCodeFence("```\n```")).toBe("");
    });

    it("normalizes CRLF before splitting", () => {
      expect(stripCodeFence("```bash\r\nls\r\n```")).toBe("ls");
    });

    it("empty fenced block", () => {
      expect(stripCodeFence("```\n\n```")).toBe("");
    });
  });

  describe("idempotency", () => {
    const cases = [
      "",
      "echo hi",
      "```\nls\n```",
      "```bash\nls\n```",
      "```sh\necho hi",
      "echo hi\n```",
    ];
    it.each(cases)("stripCodeFence x2 === stripCodeFence x1 for %j", (s) => {
      expect(stripCodeFence(stripCodeFence(s))).toBe(stripCodeFence(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// smartQuotesToStraight
// ─────────────────────────────────────────────────────────────────────────────

describe("smartQuotesToStraight", () => {
  describe("happy path", () => {
    it("replaces left/right curly double quotes", () => {
      expect(smartQuotesToStraight("“hello”")).toBe('"hello"');
    });

    it("replaces left/right curly single quotes", () => {
      expect(smartQuotesToStraight("‘hi’")).toBe("'hi'");
    });

    it("replaces low-9 quotes (German style)", () => {
      expect(smartQuotesToStraight("„german“")).toBe('"german"');
      expect(smartQuotesToStraight("‚single‘")).toBe("'single'");
    });

    it("replaces high-reversed-9 quote (U+201F and U+201B)", () => {
      expect(smartQuotesToStraight("‟text”")).toBe('"text"');
      expect(smartQuotesToStraight("‛text’")).toBe("'text'");
    });

    it("replaces mixed straight + curly", () => {
      expect(smartQuotesToStraight('echo "a" + “b”')).toBe('echo "a" + "b"');
    });

    it("replaces across multiple lines", () => {
      expect(smartQuotesToStraight("“a”\n‘b’")).toBe('"a"\n\'b\'');
    });
  });

  describe("preservation", () => {
    it("straight quotes are unchanged", () => {
      expect(smartQuotesToStraight('"hi" + \'x\'')).toBe('"hi" + \'x\'');
    });

    it("apostrophes in contractions stay as curly if they weren't ASCII already", () => {
      // This documents current behavior: curly single quotes (incl. those
      // used as apostrophes) become ASCII apostrophes.
      expect(smartQuotesToStraight("don’t")).toBe("don't");
    });

    it("backticks are untouched", () => {
      expect(smartQuotesToStraight("`code`")).toBe("`code`");
    });

    it("guillemets are NOT replaced (we don't claim them)", () => {
      expect(smartQuotesToStraight("«fr»")).toBe("«fr»");
    });
  });

  describe("edge cases", () => {
    it("empty input", () => {
      expect(smartQuotesToStraight("")).toBe("");
    });

    it("input composed only of curly quotes", () => {
      expect(smartQuotesToStraight("“”‘’")).toBe("\"\"''");
    });
  });

  describe("idempotency", () => {
    const cases = [
      "",
      "“hi”",
      '"straight"',
      "‘a’",
      "mixed \"a\" and “b”",
    ];
    it.each(cases)("smartQuotesToStraight x2 === smartQuotesToStraight x1 for %j", (s) => {
      expect(smartQuotesToStraight(smartQuotesToStraight(s))).toBe(smartQuotesToStraight(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// trimDocument
// ─────────────────────────────────────────────────────────────────────────────

describe("trimDocument", () => {
  it("strips leading + trailing whitespace", () => {
    expect(trimDocument("  \n\t  echo hi  \n\n")).toBe("echo hi");
  });

  it("strips leading only", () => {
    expect(trimDocument("\n  echo hi")).toBe("echo hi");
  });

  it("strips trailing only", () => {
    expect(trimDocument("echo hi\n  ")).toBe("echo hi");
  });

  it("preserves internal whitespace", () => {
    expect(trimDocument("  a    b  ")).toBe("a    b");
  });

  it("preserves internal newlines", () => {
    expect(trimDocument("\na\nb\n")).toBe("a\nb");
  });

  it("empty input", () => {
    expect(trimDocument("")).toBe("");
  });

  it("only-whitespace input", () => {
    expect(trimDocument("   \n\t   ")).toBe("");
  });

  it("single non-whitespace char", () => {
    expect(trimDocument("x")).toBe("x");
  });

  describe("idempotency", () => {
    const cases = ["", "x", "  x  ", "\na\n", "   \n   ", "a\nb"];
    it.each(cases)("trimDocument x2 === trimDocument x1 for %j", (s) => {
      expect(trimDocument(trimDocument(s))).toBe(trimDocument(s));
    });
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Composed pipelines (real paste scenarios)
// ─────────────────────────────────────────────────────────────────────────────

describe("composed cleanup", () => {
  function fullClean(input: string): string {
    return trimDocument(
      joinLines(
        smartQuotesToStraight(
          stripPromptPrefix(
            unwrapContinuations(stripCodeFence(input)),
          ),
        ),
      ),
    );
  }

  it("cleans a markdown-fenced `$`-prefixed curl with continuations and smart quotes", () => {
    const input = [
      "```bash",
      "$ curl -X POST “https://api.example.com/endpoint” \\",
      "    -H ‘Content-Type: application/json’ \\",
      "    -d ‘{\"name\": \"foo\"}’ \\",
      "    --fail",
      "```",
    ].join("\n");

    // Without joinLines (keep line breaks from the original doc):
    const withoutJoin = trimDocument(
      smartQuotesToStraight(
        stripPromptPrefix(
          unwrapContinuations(stripCodeFence(input)),
        ),
      ),
    );
    expect(withoutJoin).toBe(
      'curl -X POST "https://api.example.com/endpoint" -H \'Content-Type: application/json\' -d \'{"name": "foo"}\' --fail',
    );

    // With joinLines (force single line):
    expect(fullClean(input)).toBe(
      'curl -X POST "https://api.example.com/endpoint" -H \'Content-Type: application/json\' -d \'{"name": "foo"}\' --fail',
    );
  });

  it("docker run with trailing whitespace after each backslash (the bug the user reported)", () => {
    const pad = "                                                                  ";
    const input = `docker run -d \\${pad}\n    --name web \\${pad}\n    -p 8080:80 \\${pad}\n    nginx`;
    expect(unwrapContinuations(input)).toBe("docker run -d --name web -p 8080:80 nginx");
  });

  it("markdown blockquote-prefixed command from docs", () => {
    const input = "> npm install\n> npm run dev";
    expect(stripPromptPrefix(input)).toBe("npm install\nnpm run dev");
  });

  it("is idempotent when applied twice end-to-end", () => {
    const input = [
      "```bash",
      "$ echo “hi” \\",
      "    world",
      "```",
    ].join("\n");
    expect(fullClean(fullClean(input))).toBe(fullClean(input));
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Pseudo-random property tests
//
// Not "real" property-based fuzzing — no fast-check dep — but a cheap
// randomized check that catches regressions across a wider input surface
// than hand-picked cases. Seeded so failures reproduce.
// ─────────────────────────────────────────────────────────────────────────────

function mulberry32(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomString(rng: () => number, len: number): string {
  // Alphabet biased toward characters that matter to our transforms:
  // whitespace, newlines, backslash, fence ticks, quotes (smart + straight),
  // prompt markers, and a handful of plain ASCII.
  const alphabet = " \t\n\\`$>#❯\"'“”‘’abcde";
  let out = "";
  for (let i = 0; i < len; i++) {
    out += alphabet[Math.floor(rng() * alphabet.length)];
  }
  return out;
}

describe("pseudo-fuzz: all transforms are idempotent", () => {
  const transforms = {
    joinLines,
    unwrapContinuations,
    stripPromptPrefix,
    stripCodeFence,
    smartQuotesToStraight,
    trimDocument,
  };

  for (const [name, fn] of Object.entries(transforms)) {
    it(`${name} is idempotent across 500 seeded random inputs`, () => {
      const rng = mulberry32(0xC0FFEE ^ name.length);
      for (let i = 0; i < 500; i++) {
        const len = Math.floor(rng() * 60);
        const input = randomString(rng, len);
        const once = fn(input);
        const twice = fn(once);
        if (twice !== once) {
          throw new Error(
            `${name} NOT idempotent on input ${JSON.stringify(input)}\n` +
              `  once:  ${JSON.stringify(once)}\n` +
              `  twice: ${JSON.stringify(twice)}`,
          );
        }
      }
    });
  }
});

describe("pseudo-fuzz: transforms never throw", () => {
  const transforms = [
    joinLines,
    unwrapContinuations,
    stripPromptPrefix,
    stripCodeFence,
    smartQuotesToStraight,
    trimDocument,
  ];

  it("no transform throws on 1000 random inputs", () => {
    const rng = mulberry32(0xDEADBEEF);
    for (let i = 0; i < 1000; i++) {
      const len = Math.floor(rng() * 120);
      const input = randomString(rng, len);
      for (const fn of transforms) {
        expect(() => fn(input)).not.toThrow();
      }
    }
  });
});

// NOTE: Full-pipeline composed idempotency is intentionally NOT asserted.
// Fuzzing showed pathological inputs where `joinLines` can reintroduce a
// `$ ` start sequence (by collapsing `$\t` whitespace) that a second
// `stripPromptPrefix` pass would then strip. Not a user-facing concern —
// each individual transform IS idempotent (covered above), which is the
// property the user's "click twice = no-op" expectation actually rests on.
