import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import AddCardInput from "../AddCardInput.svelte";

describe("AddCardInput", () => {
  it("renders an add button", () => {
    render(AddCardInput, { onCreate: vi.fn() });
    expect(screen.getByLabelText("Add card")).toBeTruthy();
    expect(screen.getByText("Add card")).toBeTruthy();
  });

  it("calls onCreate when clicked", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(AddCardInput, { onCreate });

    await fireEvent.click(screen.getByLabelText("Add card"));

    expect(onCreate).toHaveBeenCalledTimes(1);
  });
});
