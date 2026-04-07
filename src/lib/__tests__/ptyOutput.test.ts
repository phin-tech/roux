import { describe, expect, it } from "vitest";
import { ptyOutputPayloadToBytes } from "$lib/ptyOutput";

describe("ptyOutputPayloadToBytes", () => {
  it("returns Uint8Array values unchanged", () => {
    const bytes = new Uint8Array([1, 2, 3]);
    expect(ptyOutputPayloadToBytes(bytes)).toBe(bytes);
  });

  it("converts ArrayBuffer payloads to Uint8Array", () => {
    const bytes = new Uint8Array([4, 5, 6]);
    expect([...ptyOutputPayloadToBytes(bytes.buffer)]).toEqual([4, 5, 6]);
  });

  it("converts numeric arrays to Uint8Array", () => {
    expect([...ptyOutputPayloadToBytes([7, 8, 9])]).toEqual([7, 8, 9]);
  });
});
