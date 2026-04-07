export type PtyOutputPayload = ArrayBuffer | Uint8Array | number[];

export function ptyOutputPayloadToBytes(payload: PtyOutputPayload): Uint8Array {
  if (payload instanceof Uint8Array) {
    return payload;
  }
  if (payload instanceof ArrayBuffer) {
    return new Uint8Array(payload);
  }
  return Uint8Array.from(payload);
}
