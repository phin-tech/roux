import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import {
  armPaneHints,
  armSessionHints,
  hidePaneHints,
  hideSessionHints,
  showPaneHints,
  showSessionHints,
} from "../ui";

describe("showSessionHints", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hideSessionHints();
  });

  afterEach(() => {
    hideSessionHints();
    vi.useRealTimers();
  });

  it("stays false until the hold delay elapses", () => {
    armSessionHints(200);
    expect(get(showSessionHints)).toBe(false);
    vi.advanceTimersByTime(199);
    expect(get(showSessionHints)).toBe(false);
    vi.advanceTimersByTime(1);
    expect(get(showSessionHints)).toBe(true);
  });

  it("hide cancels a pending arm", () => {
    armSessionHints(200);
    vi.advanceTimersByTime(100);
    hideSessionHints();
    vi.advanceTimersByTime(200);
    expect(get(showSessionHints)).toBe(false);
  });

  it("hide turns off an already-visible overlay", () => {
    armSessionHints(50);
    vi.advanceTimersByTime(50);
    expect(get(showSessionHints)).toBe(true);
    hideSessionHints();
    expect(get(showSessionHints)).toBe(false);
  });

  it("arming again while a timer is pending is a no-op", () => {
    armSessionHints(200);
    vi.advanceTimersByTime(150);
    armSessionHints(200); // should not reset the timer
    vi.advanceTimersByTime(50);
    expect(get(showSessionHints)).toBe(true);
  });
});

describe("showPaneHints", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hidePaneHints();
  });

  afterEach(() => {
    hidePaneHints();
    vi.useRealTimers();
  });

  it("has independent timer state from the session hint store", () => {
    armSessionHints(200);
    armPaneHints(200);
    vi.advanceTimersByTime(200);
    expect(get(showSessionHints)).toBe(true);
    expect(get(showPaneHints)).toBe(true);
    hideSessionHints();
    expect(get(showSessionHints)).toBe(false);
    expect(get(showPaneHints)).toBe(true);
  });
});
