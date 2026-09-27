// The paired Hosts, the `hosts` section of settings.json (src/lib/settings/store.ts): each one's
// URL, the token the Host gave this Mac at pairing, and its name as last heard. Plus the pure
// rules for a Host's URL. See docs/architecture.md "Hosts". No state: src/lib/host/hosts.svelte.ts.

export interface PairedHost {
  /** This client's id for the Host (`h_<hex>`): what a Tab's `host` names. */
  id: string;
  /** Where the Host's Remote server is: `https://dell.tail1234.ts.net` or `http://127.0.0.1:47611`. */
  url: string;
  /** The token the Host handed over at pairing; sent first on every connection. */
  token: string;
  /** The Host's name from its last `hello`, for the section header while it is offline. */
  name: string | null;
}

/** Defensive parse of the section: unknown JSON, possibly stale or hand-edited. */
export function parseHostsSection(raw: unknown): PairedHost[] {
  if (!Array.isArray(raw)) return [];
  const out: PairedHost[] = [];
  const seen = new Set<string>();
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const r = item as Record<string, unknown>;
    if (typeof r.id !== "string" || r.id === "" || seen.has(r.id)) continue;
    const url = typeof r.url === "string" ? normalizeHostUrl(r.url) : null;
    if (!url || typeof r.token !== "string" || r.token === "") continue;
    seen.add(r.id);
    out.push({ id: r.id, url, token: r.token, name: typeof r.name === "string" && r.name !== "" ? r.name : null });
  }
  return out;
}

export function newHostId(): string {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return "h_" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * A Host's base URL as typed: `dell.tail1234.ts.net` gets `https://`, a trailing slash or the
 * phone page's `/m` (a pasted pairing link) is dropped, and anything that is not an http(s)
 * origin is refused (null). `mock://<name>` is the browser mock's fake Host (src/lib/mock.ts).
 */
export function normalizeHostUrl(input: string): string | null {
  let text = input.trim();
  if (text === "") return null;
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = `https://${text}`;
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return null;
  }
  if (url.protocol === "mock:") return url.host ? `mock://${url.host}` : null;
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  if (url.username || url.password) return null;
  const path = url.pathname.replace(/\/+$/, "");
  if (path !== "" && path !== "/m") return null;
  return url.origin;
}

/** The pairing code in a pasted pairing link (`…/m#pair=CODE`), if any. */
export function codeFromPairingLink(input: string): string | null {
  const m = /#pair=([A-Za-z0-9]+)/.exec(input);
  return m ? m[1] : null;
}

/** The connection's URL for a Host's base URL: `wss://…/ws` (or `ws://` for an http Host). */
export function webSocketUrl(base: string): string {
  const url = new URL(base);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/ws";
  url.search = "";
  url.hash = "";
  return url.toString();
}
