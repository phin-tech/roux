import { derived, get, type Readable } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { EXPERIMENT_DEFAULTS } from "$lib/types";
import type { ExperimentsConfig } from "$lib/bindings";

type RequiredExperiments = Required<ExperimentsConfig>;
type GeneratedExperimentId = string extends keyof RequiredExperiments
  ? never
  : keyof RequiredExperiments;

type BooleanExperimentId = {
  [K in GeneratedExperimentId]: RequiredExperiments[K] extends boolean ? K : never;
}[GeneratedExperimentId];

type EnumExperimentId = Exclude<GeneratedExperimentId, BooleanExperimentId>;

type BooleanExperimentDef<K extends BooleanExperimentId = BooleanExperimentId> = {
  kind: "boolean";
  id: K;
  label: string;
  description: string;
};

type EnumExperimentDef<K extends EnumExperimentId = EnumExperimentId> = {
  kind: "enum";
  id: K;
  label: string;
  description: string;
  options: ReadonlyArray<{ value: RequiredExperiments[K]; label: string }>;
};

type RegisteredExperimentDef =
  | BooleanExperimentDef
  | EnumExperimentDef;

type EmptyExperimentDef =
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

export type ExperimentDef = [GeneratedExperimentId] extends [never]
  ? EmptyExperimentDef
  : RegisteredExperimentDef;

type ExperimentRegistry = [GeneratedExperimentId] extends [never]
  ? Record<string, never>
  : { [K in GeneratedExperimentId]:
      K extends BooleanExperimentId
        ? BooleanExperimentDef<K>
        : K extends EnumExperimentId
          ? EnumExperimentDef<K>
          : never
    };

// Indexed by id so adding a new flag to `ExperimentsConfig` (Rust side) without
// adding a registry entry here is a TypeScript error. The `Record<string, never>`
// branch matches specta's empty-struct binding while there are no experiments.
const EXPERIMENT_DEFS = {} satisfies ExperimentRegistry;

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
  return Boolean(readExperimentValue(id));
}

export function getExperimentValue<T = unknown>(id: string): T {
  return readExperimentValue(id) as T;
}

export function readExperimentValue(id: string): unknown {
  return readExperiments()[id as keyof RequiredExperiments];
}

export function currentExperimentValue(
  experiments: ExperimentsConfig | undefined,
  id: string,
): unknown {
  return { ...EXPERIMENT_DEFAULTS, ...(experiments ?? {}) }[
    id as keyof RequiredExperiments
  ];
}

export function withExperimentValue(
  experiments: ExperimentsConfig | undefined,
  id: string,
  value: unknown,
): ExperimentsConfig {
  return { ...(experiments ?? {}), [id]: value } as ExperimentsConfig;
}
