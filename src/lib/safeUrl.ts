// URL-sanitization helper for external-origin URLs rendered in `<a href>`.
//
// `wt list --format=json` can surface arbitrary strings in `ci.url` and
// the top-level `url` (dev server). A compromised / malicious wt output
// could therefore pass `javascript:` / `data:` / `file:` URIs that
// would execute in the WebView context if we bind them directly to
// `href`. This helper returns `null` for anything that isn't plain
// http(s), so template guards (`{#if safeHref(x)}`) short-circuit
// rendering entirely for unsafe values.

export function safeHref(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const u = new URL(url);
    if (u.protocol === "http:" || u.protocol === "https:") return url;
    return null;
  } catch {
    return null;
  }
}
