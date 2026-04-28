export function paneAtPoint(x: number, y: number): string | null {
  const target = document.elementFromPoint(x, y);
  if (!target) return null;
  const match = target.closest<HTMLElement>("[data-pane-id]");
  const id = match?.getAttribute("data-pane-id");
  return id && id.length > 0 ? id : null;
}
