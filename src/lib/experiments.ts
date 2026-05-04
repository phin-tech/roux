import { derived, get, type Readable } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { EXPERIMENT_DEFAULTS } from "$lib/types";
import type { ExperimentsConfig } from "$lib/bindings";

type RequiredExperiments = Required<ExperimentsConfig>;

type BoolExperimentId = {
  [K in keyof RequiredExperiments]: RequiredExperiments[K] extends boolean ? K : never;
}[keyof RequiredExperiments];

type EnumExperimentId = Exclude<keyof RequiredExperiments, BoolExperimentId>;

type ExperimentDefFor<K extends keyof RequiredExperiments> =
  RequiredExperiments[K] extends boolean
    ? { kind: "boolean"; id: K; label: string; description: string }
    : {
        kind: "enum";
        id: K;
        label: string;
        description: string;
        options: ReadonlyArray<{ value: RequiredExperiments[K]; label: string }>;
      };

export type ExperimentDef =
  | ExperimentDefFor<BoolExperimentId>
  | ExperimentDefFor<EnumExperimentId>;

// Indexed by id so adding a new flag to `ExperimentsConfig` (Rust side) without
// adding a registry entry here is a TypeScript error — the UI can't silently
// miss a flag.
const EXPERIMENT_DEFS: { [K in keyof RequiredExperiments]: ExperimentDefFor<K> } = {
  exampleFlag: {
    kind: "boolean",
    id: "exampleFlag",
    label: "Example flag",
    description:
      "No-op flag for verifying the boolean experiments pipeline. Safe to remove once a real experiment lands.",
  },
  exampleVariant: {
    kind: "enum",
    id: "exampleVariant",
    label: "Example variant",
    description:
      "No-op multi-choice flag for verifying the enum experiments pipeline. Safe to remove once a real experiment lands.",
    options: [
      { value: "a", label: "Variant A" },
      { value: "b", label: "Variant B" },
      { value: "c", label: "Variant C" },
    ],
  },
  simplifiedSessionTabs: {
    kind: "boolean",
    id: "simplifiedSessionTabs",
    label: "Simplified session tabs",
    description:
      "Replace the session sidebar's per-tab metadata chips with a single contextual line (worktree or repo name, depending on the current grouping).",
  },
};

export const EXPERIMENTS: ReadonlyArray<ExperimentDef> = Object.values(
  EXPERIMENT_DEFS,
) as ExperimentDef[];

export { EXPERIMENT_DEFAULTS };

function readExperiments(): RequiredExperiments {
  return { ...EXPERIMENT_DEFAULTS, ...(get(settings).experiments ?? {}) };
}

// Reactive view of the resolved experiment values. Use this from Svelte
// components when the UI should respond live to flag toggles in Settings →
// Experiments. Non-reactive callers (event handlers, one-shot reads) should
// keep using `isExperimentEnabled` / `getExperimentValue`.
export const experimentValues: Readable<RequiredExperiments> = derived(
  settings,
  ($s) => ({ ...EXPERIMENT_DEFAULTS, ...($s.experiments ?? {}) }),
);

export function isExperimentEnabled(id: BoolExperimentId): boolean {
  return readExperiments()[id];
}

export function getExperimentValue<K extends keyof RequiredExperiments>(
  id: K,
): RequiredExperiments[K] {
  return readExperiments()[id];
}
