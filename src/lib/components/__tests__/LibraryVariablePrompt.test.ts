import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { get } from "svelte/store";
import { afterEach, describe, expect, it } from "vitest";
import LibraryVariablePrompt from "../LibraryVariablePrompt.svelte";
import {
  cancelLibraryVariablePrompt,
  libraryVariablePrompt,
  requestLibraryVariables,
} from "$lib/stores/libraryVariablePrompt";

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

describe("LibraryVariablePrompt", () => {
  afterEach(() => {
    cancelLibraryVariablePrompt();
  });

  it("keeps focus on the field being edited", async () => {
    const promise = requestLibraryVariables({
      title: "Test prompt",
      variables: [
        { name: "first", label: "First", default: null, required: true },
        { name: "second", label: "Second", default: null, required: true },
      ],
    });
    render(LibraryVariablePrompt);

    const inputs = await waitFor(() => {
      const all = document.querySelectorAll<HTMLInputElement>("input");
      expect(all).toHaveLength(2);
      return Array.from(all);
    });

    await nextFrame();
    inputs[1].focus();
    expect(document.activeElement).toBe(inputs[1]);

    await fireEvent.input(inputs[1], { target: { value: "typed" } });
    await nextFrame();

    expect(document.activeElement).toBe(inputs[1]);
    cancelLibraryVariablePrompt();
    await expect(promise).resolves.toBeNull();
  });

  it("renders typed controls and validates numeric variables", async () => {
    const promise = requestLibraryVariables({
      title: "Typed prompt",
      variables: [
        {
          name: "count",
          label: "Count",
          default: null,
          required: true,
          valueType: "int",
          options: [],
        },
        {
          name: "temperature",
          label: "Temperature",
          default: "0.5",
          required: false,
          valueType: "float",
          options: [],
        },
        {
          name: "tone",
          label: "Tone",
          default: "friendly",
          required: true,
          valueType: "select",
          options: ["friendly", "direct"],
        },
      ],
    });
    render(LibraryVariablePrompt);

    const count = await waitFor(() => {
      const input = document.querySelector<HTMLInputElement>("input[name='count']");
      expect(input).toBeTruthy();
      return input!;
    });
    const temperature = document.querySelector<HTMLInputElement>("input[name='temperature']");
    const tone = document.querySelector<HTMLSelectElement>("select[name='tone']");

    expect(count.type).toBe("number");
    expect(count.step).toBe("1");
    expect(temperature?.type).toBe("number");
    expect(temperature?.step).toBe("any");
    expect(tone).toBeTruthy();
    expect(Array.from(tone!.options).map((option) => option.value)).toEqual([
      "friendly",
      "direct",
    ]);

    await fireEvent.input(count, { target: { value: "1.5" } });
    await fireEvent.click(document.querySelector<HTMLButtonElement>("button[type='button']:last-child")!);

    await waitFor(() => {
      expect(document.body.textContent).toContain("Count must be an integer.");
    });

    await fireEvent.input(count, { target: { value: "3" } });
    await fireEvent.change(tone!, { target: { value: "direct" } });
    await fireEvent.click(document.querySelector<HTMLButtonElement>("button[type='button']:last-child")!);

    await expect(promise).resolves.toEqual({
      count: "3",
      temperature: "0.5",
      tone: "direct",
    });
  });

  it("closes a superseded prompt before returning zero-variable values", async () => {
    const first = requestLibraryVariables({
      title: "First",
      variables: [{ name: "value", label: "Value", default: null, required: true }],
    });
    render(LibraryVariablePrompt);

    await waitFor(() => {
      expect(get(libraryVariablePrompt).open).toBe(true);
    });

    const second = requestLibraryVariables({
      title: "Second",
      variables: [],
      initialValues: { ready: "yes" },
    });

    await expect(first).resolves.toBeNull();
    await expect(second).resolves.toEqual({ ready: "yes" });
    expect(get(libraryVariablePrompt).open).toBe(false);
  });
});
