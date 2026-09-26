// The paired Hosts, the `hosts` section of settings.json (src/lib/settings/store.ts): each one's
// URL, the token the Host gave this Mac at pairing, its name as last heard, and its repo-to-path
// map for Handoff (a Checkout root plus per-repo overrides). Plus the pure rules for a Host's
// URL and for where a repo is on a Host. See docs/architecture.md "Hosts" and "Handoff". No
// state: src/lib/host/hosts.svelte.ts.

export interface PairedHost {
  /** This client's id for the Host (`h_<hex>`): what a Tab's `host` names. */
  id: string;
  /** Where the Host's Remote server is: `https://dell.tail1234.ts.net` or `http://127.0.0.1:47611`. */
  url: string;
  /** The token the Host handed over at pairing; sent first on every connection. */
  token: string;
  /** The Host's name from its last `hello`, for the section header while it is offline. */
  name: string | null;
  /**
   * The Checkout root: the directory on the Host under which its repos are checked out, so the
   * repo `x` is at `<root>/x` there (`~` for the Host's home). null until the user sets it.
   */
  checkoutRoot: string | null;
  /** Per-repo overrides: repo name (as the Badge shows it) to its checkout's path on the Host. */
  repoPaths: Record<string, string>;
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
    out.push({
      id: r.id,
      url,
      token: r.token,
      name: typeof r.name === "string" && r.name !== "" ? r.name : null,
      checkoutRoot: cleanPath(r.checkoutRoot),
      repoPaths: parseRepoPaths(r.repoPaths),
    });
  }
  return out;
}

/** A path as typed for a Host: trimmed, a trailing slash dropped; null when empty or not a string. */
export function cleanPath(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const text = raw.trim().replace(/\/+$/, "");
  return text === "" ? null : text.startsWith("/") || text === "~" || text.startsWith("~/") ? text : null;
}

function parseRepoPaths(raw: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return out;
  for (const [repo, path] of Object.entries(raw as Record<string, unknown>)) {
    const clean = cleanPath(path);
    if (repo.trim() !== "" && clean) out[repo.trim()] = clean;
  }
  return out;
}

/**
 * Where a repo is checked out on a Host: its override, else `<Checkout root>/<repo>`; null when
 * neither is set or the Tab is in no repo. `~` stands for the Host's home (`hello.host.home`);
 * when that is unknown the path is left as typed and the Host's shell expands nothing, so the
 * caller treats it as unset.
 */
export function repoPathOnHost(host: Pick<PairedHost, "checkoutRoot" | "repoPaths">, repo: string | null, hostHome: string | null): string | null {
  if (!repo) return null;
  const raw = host.repoPaths[repo] ?? (host.checkoutRoot ? `${host.checkoutRoot}/${repo}` : null);
  if (!raw) return null;
  return expandHome(raw, hostHome);
}

/** `~` and `~/x` with the Host's home; null when the home is needed and unknown. */
export function expandHome(path: string, hostHome: string | null): string | null {
  if (path === "~") return hostHome;
  if (path.startsWith("~/")) return hostHome ? `${hostHome.replace(/\/+$/, "")}${path.slice(1)}` : null;
  return path;
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
