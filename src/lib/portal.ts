import type { Action } from "svelte/action";

/**
 * Move a node into a target element (default: `document.body`) on mount,
 * remove it on destroy. Useful for popovers/tooltips inside scroll
 * containers — `overflow: hidden/auto` ancestors clip absolutely
 * positioned children, but `position: fixed` descendants of
 * `document.body` are anchored to the viewport and escape that clipping.
 *
 * Callers are responsible for positioning the portaled node themselves
 * (typically via `position: fixed` with coordinates derived from an
 * anchor element's bounding rect).
 */
export const portal: Action<HTMLElement, HTMLElement | undefined | null> = (
  node,
  target,
) => {
  const dest = target ?? document.body;
  dest.appendChild(node);
  return {
    update(next) {
      const nextDest = next ?? document.body;
      if (nextDest !== node.parentElement) {
        nextDest.appendChild(node);
      }
    },
    destroy() {
      node.remove();
    },
  };
};
