import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import {
  customProfileModalState,
  openCustomProfileEditor,
  submitCustomProfile,
  closeCustomProfileEditor,
} from "../customProfileModal";
import type { SpawnProfile } from "$lib/panes/profiles";

function sampleProfile(overrides: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id: "inline-test",
    name: "Test",
    source: "inline",
    startupCommand: "echo hi",
    ...overrides,
  };
}

describe("customProfileModal store", () => {
  beforeEach(() => {
    // Ensure any lingering pending resolver from a prior test is cleared.
    closeCustomProfileEditor();
  });

  it("starts closed", () => {
    expect(get(customProfileModalState).visible).toBe(false);
  });

  it("opens visible and resolves with the submitted profile", async () => {
    const promise = openCustomProfileEditor();
    expect(get(customProfileModalState).visible).toBe(true);

    const profile = sampleProfile();
    submitCustomProfile(profile);

    await expect(promise).resolves.toEqual(profile);
    expect(get(customProfileModalState).visible).toBe(false);
  });

  it("resolves with null when the user cancels", async () => {
    const promise = openCustomProfileEditor();
    closeCustomProfileEditor();

    await expect(promise).resolves.toBeNull();
    expect(get(customProfileModalState).visible).toBe(false);
  });

  it("preempts the pending call when open is fired a second time", async () => {
    // A hypothetical double-fire from the palette shouldn't strand the
    // first promise — the first caller gets null, the second caller
    // gets the submitted profile.
    const first = openCustomProfileEditor();
    const second = openCustomProfileEditor();

    const profile = sampleProfile({ id: "inline-2", name: "Second" });
    submitCustomProfile(profile);

    await expect(first).resolves.toBeNull();
    await expect(second).resolves.toEqual(profile);
  });
});
