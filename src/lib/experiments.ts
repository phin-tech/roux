import { get } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { DEFAULT_SETTINGS } from "$lib/types";
import type { ExperimentsConfig } from "$lib/bindings";

type RequiredExperiments = Required<ExperimentsConfig>;

const DEFAULTS = DEFAULT_SETTINGS.experiments as RequiredExperiments;

type BoolExperimentId = {
  [K in keyof RequiredExperiments]: RequiredExperiments[K] extends boolean ? K : never;
}[keyof RequiredExperiments];

type EnumExperimentId = Exclude<keyof RequiredExperiments, BoolExperimentId>;

type BoolExperimentDef = {
  kind: "boolean";
  id: BoolExperimentId;
  label: string;
  description: string;
};

type EnumExperimentDef = {
  [K in EnumExperimentId]: {
    kind: "enum";
    id: K;
    label: string;
    description: string;
    options: ReadonlyArray<{ value: RequiredExperiments[K]; label: string }>;
  };
}[EnumExperimentId];

export type ExperimentDef = BoolExperimentDef | EnumExperimentDef;

export const EXPERIMENTS: ReadonlyArray<ExperimentDef> = [
  {
    kind: "boolean",
    id: "exampleFlag",
    label: "Example flag",
    description:
      "No-op flag for verifying the boolean experiments pipeline. Safe to remove once a real experiment lands.",
  },
  {
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
];

function readExperiments(): RequiredExperiments {
  return { ...DEFAULTS, ...(get(settings).experiments ?? {}) };
}

export function isExperimentEnabled(id: BoolExperimentId): boolean {
  return readExperiments()[id];
}

export function getExperimentValue<K extends keyof RequiredExperiments>(
  id: K,
): RequiredExperiments[K] {
  return readExperiments()[id];
}
