# Nono Wrapping in Layouts + Remove Legacy Claude Path

## Summary

Unify session creation onto a single spawn path (`createSessionShell` + `runProfileInPane`) and move nono sandbox wrapping to the shell-spawn layer so it works for all profiles and layouts — not just the legacy Claude built-in.

## Goals

1. **Nono works in layouts.** A leaf pane can specify `nono="profile-name"` and optional `nono_flags { allow_dir "..." }` to sandbox the shell.
2. **Nono works for any profile**, not just Claude. The nono dropdown in `NewSessionDialog` is ungated from `useLegacyClaudePath`.
3. **Remove the legacy Claude spawn path.** `createSession` (the one that launches the Claude binary directly in a PTY) is deleted. All sessions use `createSessionShell` → `runProfileInPane`. The built-in Claude provider module already builds the correct `startup_command` from settings.
4. **`--dangerously-skip-permissions` becomes a profile concern**, not a dialog checkbox. Users who want it create a profile with the flag in `startup_command` or `additional_flags`.
5. **Continue/Resume/New UX preserved.** The SessionPicker component and `listClaudeSessions` stay. The extra flags (`--continue`, `--resume <id>`) are appended to the startup command typed into the shell, instead of being passed as spawn args.

## Spec and implementation plan

See `docs/plans/2026-04-11-nono-layouts-and-legacy-removal-impl.md` for the full implementation plan.
