import type { DropSide } from "./layout";

type RectLike = Pick<DOMRect, "left" | "top" | "width" | "height">;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Resolve which side of a pane the pointer is closest to.
 * The result is always stable, even for very small panes.
 */
export function getDropSide(
  rect: RectLike,
  clientX: number,
  clientY: number
): DropSide {
  const width = Math.max(rect.width, 1);
  const height = Math.max(rect.height, 1);
  const x = clamp(clientX - rect.left, 0, width);
  const y = clamp(clientY - rect.top, 0, height);

  const distances: Array<{ side: DropSide; distance: number }> = [
    { side: "left", distance: x },
    { side: "right", distance: width - x },
    { side: "top", distance: y },
    { side: "bottom", distance: height - y },
  ];

  distances.sort((a, b) => a.distance - b.distance);
  return distances[0].side;
}
