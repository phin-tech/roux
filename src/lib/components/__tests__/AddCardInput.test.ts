import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import AddCardInput from "../AddCardInput.svelte";

describe("AddCardInput", () => {
  it("reveals an input when the add button is clicked", async () => {
    render(AddCardInput, { onCreate: vi.fn() });
    expect(screen.queryByLabelText("New card title")).toBeNull();

    await fireEvent.click(screen.getByLabelText("Add card"));
    expect(screen.getByLabelText("New card title")).toBeTruthy();
  });

  it("creates on Enter with a trimmed title and clears for the next", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(AddCardInput, { onCreate });

    await fireEvent.click(screen.getByLabelText("Add card"));
    const input = screen.getByLabelText("New card title") as HTMLInputElement;

    await fireEvent.input(input, { target: { value: "  Wire the board  " } });
    await fireEvent.keyDown(input, { key: "Enter" });

    expect(onCreate).toHaveBeenCalledWith("Wire the board");
    // Stays open and clears for rapid entry.
    expect(screen.getByLabelText("New card title")).toBeTruthy();
    expect((screen.getByLabelText("New card title") as HTMLInputElement).value).toBe("");
  });

  it("ignores an empty/whitespace title", async () => {
    const onCreate = vi.fn();
    render(AddCardInput, { onCreate });

    await fireEvent.click(screen.getByLabelText("Add card"));
    const input = screen.getByLabelText("New card title");
    await fireEvent.input(input, { target: { value: "   " } });
    await fireEvent.keyDown(input, { key: "Enter" });

    expect(onCreate).not.toHaveBeenCalled();
  });

  it("collapses on Escape", async () => {
    render(AddCardInput, { onCreate: vi.fn() });
    await fireEvent.click(screen.getByLabelText("Add card"));
    const input = screen.getByLabelText("New card title");

    await fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByLabelText("New card title")).toBeNull();
    expect(screen.getByLabelText("Add card")).toBeTruthy();
  });
});
