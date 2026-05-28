import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import RepoAutoComplete from "../RepoAutoComplete.svelte";

const options = [
  { path: "/repos/alpha", label: "alpha" },
  { path: "/repos/beta", label: "beta" },
];

function getOptionItems(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[role="option"]'));
}

function getInput(): HTMLInputElement {
  const input = document.querySelector<HTMLInputElement>("input");
  if (!input) throw new Error("RepoAutoComplete input not rendered");
  return input;
}

describe("RepoAutoComplete focus suppression after selection", () => {
  it("keeps the dropdown closed until the picker is active", async () => {
    render(RepoAutoComplete, {
      props: {
        value: "",
        options,
        hasConfiguredRoots: true,
        onselect: vi.fn(),
      },
    });

    expect(getOptionItems().length).toBe(0);

    await fireEvent.focus(getInput());
    await waitFor(() => expect(getOptionItems().length).toBe(2));
  });

  it("does not reopen the dropdown when the caller programmatically refocuses the input after a selection", async () => {
    const onselect = vi.fn();
    render(RepoAutoComplete, {
      props: {
        value: "",
        options,
        hasConfiguredRoots: true,
        onselect,
      },
    });

    await fireEvent.focus(getInput());
    await waitFor(() => expect(getOptionItems().length).toBe(2));

    await fireEvent.click(getOptionItems()[0]);
    expect(onselect).toHaveBeenCalledWith("/repos/alpha", "alpha");
    await waitFor(() => expect(getOptionItems().length).toBe(0));

    // Reproduces NewSessionDialog.focusDirectoryInput() — programmatic focus
    // returning to the input after selection. Pre-fix, this reopened the dropdown.
    await fireEvent.focus(getInput());
    expect(getOptionItems().length).toBe(0);
  });

  it("reopens the dropdown on a subsequent user-initiated refocus", async () => {
    render(RepoAutoComplete, {
      props: {
        value: "",
        options,
        hasConfiguredRoots: true,
        onselect: vi.fn(),
      },
    });

    await fireEvent.focus(getInput());
    await waitFor(() => expect(getOptionItems().length).toBe(2));

    await fireEvent.click(getOptionItems()[0]);
    await waitFor(() => expect(getOptionItems().length).toBe(0));

    const input = getInput();

    // First focus consumes the one-shot guard.
    await fireEvent.focus(input);
    expect(getOptionItems().length).toBe(0);

    // Second focus is a normal user re-focus and should reopen.
    await fireEvent.focus(input);
    await waitFor(() => expect(getOptionItems().length).toBeGreaterThan(0));
  });

  it("reopens the dropdown when the user types after a selection", async () => {
    render(RepoAutoComplete, {
      props: {
        value: "",
        options,
        hasConfiguredRoots: true,
        onselect: vi.fn(),
      },
    });

    await fireEvent.focus(getInput());
    await waitFor(() => expect(getOptionItems().length).toBe(2));

    await fireEvent.click(getOptionItems()[0]);
    await waitFor(() => expect(getOptionItems().length).toBe(0));

    await fireEvent.input(getInput(), { target: { value: "alp" } });
    await waitFor(() => expect(getOptionItems().length).toBeGreaterThan(0));
  });
});
