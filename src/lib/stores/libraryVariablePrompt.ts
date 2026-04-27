import { writable } from "svelte/store";
import type { LibraryVariable, LibraryVariableType } from "$lib/tauri";

export interface LibraryVariablePromptRequest {
  title: string;
  variables: LibraryVariable[];
  initialValues?: Record<string, string>;
}

interface LibraryVariablePromptState {
  open: boolean;
  title: string;
  variables: LibraryVariable[];
  values: Record<string, string>;
  errors: Record<string, string>;
}

const INITIAL_STATE: LibraryVariablePromptState = {
  open: false,
  title: "",
  variables: [],
  values: {},
  errors: {},
};

let resolver: ((values: Record<string, string> | null) => void) | null = null;

export const libraryVariablePrompt = writable<LibraryVariablePromptState>(INITIAL_STATE);

export function requestLibraryVariables(
  request: LibraryVariablePromptRequest,
): Promise<Record<string, string> | null> {
  if (resolver) {
    resolver(null);
    resolver = null;
    libraryVariablePrompt.set(INITIAL_STATE);
  }

  if (request.variables.length === 0) {
    return Promise.resolve(request.initialValues ?? {});
  }

  const values = { ...(request.initialValues ?? {}) };
  for (const variable of request.variables) {
    values[variable.name] ??= initialValueForVariable(variable);
  }
  libraryVariablePrompt.set({
    open: true,
    title: request.title,
    variables: request.variables,
    values,
    errors: {},
  });

  return new Promise((resolve) => {
    resolver = resolve;
  });
}

export function submitLibraryVariableForm(): void {
  libraryVariablePrompt.update((state) => {
    if (!state.open) return state;
    const errors = validateLibraryVariableValues(state.variables, state.values);
    if (Object.keys(errors).length > 0) return { ...state, errors };
    resolver?.(state.values);
    resolver = null;
    return INITIAL_STATE;
  });
}

export function setLibraryVariableValue(name: string, value: string): void {
  libraryVariablePrompt.update((state) => {
    const errors = { ...state.errors };
    delete errors[name];
    return {
      ...state,
      values: { ...state.values, [name]: value },
      errors,
    };
  });
}

export function cancelLibraryVariablePrompt(): void {
  resolver?.(null);
  resolver = null;
  libraryVariablePrompt.set(INITIAL_STATE);
}

export function validateLibraryVariableValues(
  variables: LibraryVariable[],
  values: Record<string, string>,
): Record<string, string> {
  const errors: Record<string, string> = {};
  for (const variable of variables) {
    const value = values[variable.name] ?? "";
    if (variable.required && value.trim() === "") {
      errors[variable.name] = `${variable.label ?? variable.name} is required.`;
      continue;
    }
    if (value.trim() === "") {
      continue;
    }
    const type = variableType(variable);
    const label = variable.label ?? variable.name;
    if (type === "int" && !isIntegerString(value)) {
      errors[variable.name] = `${label} must be an integer.`;
    } else if (type === "float" && !isFiniteNumberString(value)) {
      errors[variable.name] = `${label} must be a number.`;
    } else if (type === "select" && !selectOptions(variable).includes(value)) {
      errors[variable.name] = `${label} must be one of the listed options.`;
    }
  }
  return errors;
}

export function initialLibraryVariableValue(variable: LibraryVariable): string {
  return initialValueForVariable(variable);
}

export function libraryVariableType(variable: LibraryVariable): LibraryVariableType {
  return variableType(variable);
}

function variableType(variable: LibraryVariable): LibraryVariableType {
  return variable.valueType ?? "string";
}

function initialValueForVariable(variable: LibraryVariable): string {
  if (variable.default != null) return variable.default;
  if (variableType(variable) === "select" && variable.required) {
    return selectOptions(variable)[0] ?? "";
  }
  return "";
}

function selectOptions(variable: LibraryVariable): string[] {
  return variable.options ?? [];
}

function isIntegerString(value: string): boolean {
  return /^[-+]?\d+$/.test(value.trim());
}

function isFiniteNumberString(value: string): boolean {
  if (value.trim() === "") return false;
  const parsed = Number(value);
  return Number.isFinite(parsed);
}
