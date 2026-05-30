import { derived, get, type Readable } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { EXPERIMENT_DEFAULTS } from "$lib/types";
import type { ExperimentsConfig } from "$lib/bindings";

type RequiredExperiments = Required<ExperimentsConfig>;

export type ExperimentDef =
  | {
      kind: "boolean";
      id: string;
      label: string;
      description: string;
    }
  | {
      kind: "enum";
      id: string;
      label: string;
      description: string;
      options: ReadonlyArray<{ value: string; label: string }>;
    };

// Adding a new flag to `ExperimentsConfig` (Rust side) should be accompanied by
// a new entry here so the UI knows how to render it.
const EXPERIMENT_DEFS: Record<string, ExperimentDef> = {};

export const EXPERIMENTS: ReadonlyArray<ExperimentDef> = Object.values(EXPERIMENT_DEFS);

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

export function isExperimentEnabled(id: string): boolean {
  return Boolean((readExperiments() as Record<string, unknown>)[id]);
}

export function getExperimentValue<T = unknown>(id: string): T {
  return (readExperiments() as Record<string, unknown>)[id] as T;
}
