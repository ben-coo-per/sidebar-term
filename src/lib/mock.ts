// Browser-only fake backend so the UI runs under plain `vite dev` without Rust.
// Each fake Session is a tiny line-echo "shell". Typing one of these commands changes what
// the fake monitor reports, so every sidebar state can be exercised by hand:
//   claude | codex | gemini   -> Agent session (type `exit` to return to the shell)
//   ssh                       -> remote hop
//   npm | pnpm | uv ...       -> a long-running command (`exit` or Ctrl-C to stop it)
//   cd <path>                 -> cwd; paths under the fake repos below get git info
//   cd ~/Dev/jack             -> main Worktree on `main`
//   cd ~/Dev/jack/.claude/worktrees/navbar -> linked Worktree `navbar` on `navbar-new-gift`
//   cd ~/Dev/detached         -> detached HEAD
//   ls                        -> file paths to double-click (opening one logs it to the console)
//   offline                   -> (on the fake remote Host only) drop its connection for a while
// The layout (Groups, Tabs, the active Tab) is a copy of the Host's rules (src-tauri/core/src/layout/)
// kept in localStorage. Activity is invented: each fake Session has a shell (and
// its foreground program, busy when it is an agent) next to a fixed cast of jittering system
// processes. Usage is invented too: fixed limits, Claude Code's 5-hour window creeping up.
// Caffeinate is only a flag: nothing is kept awake. Memory Guard runs its policy on the invented
// memory (each running agent adds 3.5 GB of 16 GB): open two agent Tabs to see one frozen, and
// `exit` one to see it thawed. Frozen Sessions are only marked, nothing stops. Resume is kept in localStorage like Rust's
// resume.json: start `claude` or `npm run dev` in a Tab, reload the page, and the banner offers it.
//
// A fake Host is one instance of `createFakeHost`: the local one behind the IPC exports, and a
// second, "dell", behind `hostClient` (a fake Host protocol connection, src/lib/host/connect.ts),
// so the sidebar's Host section shows with `pnpm dev`. Its layout is its own localStorage key.

import type { TabNewOptions } from "./ipc";
import type { ClientEvents, HostClient, Paired } from "./host/client";
import type { Command, CommandResult } from "./host/protocol";
import type {
  ActivityProcess,
  ActivitySession,
  ActivitySnapshot,
  AgentKind,
  AgentStatus,
  AgentUsage,
  GitInfo,
  Group,
  HostInfo,
  LayoutSnapshot,
  Pairing,
  RemoteSnapshot,
  GuardSnapshot,
  ResumeEntry,
  SessionExit,
  SessionId,
  SessionInfo,
  Tab,
  UsageSnapshot,
} from "./types";

type InfoCb = (i: SessionInfo) => void;
type ExitCb = (e: SessionExit) => void;
type ActivityCb = (a: ActivitySnapshot) => void;
type LayoutCb = (s: LayoutSnapshot) => void;

interface FakeSession {
  id: SessionId;
  /** The attached Terminal's sink; output before one attaches is held. */
  onData: ((b: Uint8Array) => void) | null;
  held: Uint8Array[];
  /** Recent output, for a Host protocol replay on attach (the Host's tap ring). */
  ring: Uint8Array[];
  ringBytes: number;
  line: string;
  cwd: string;
  fg: string;
  agent: AgentKind | null;
  remote: boolean;
  /** The command line of a fake long-running command, while one runs. */
  command: string | null;
  resumeKey: string | null;
  /** The latest fake OSC title, and the fake agent's status (the Host derives the real one). */
  title: string | null;
  bells: number;
  status: AgentStatus | null;
  /** The timer that ends a fake agent's "running" spell. */
  busy: ReturnType<typeof setTimeout> | null;
}

const HOME = "/Users/you";
const RING_MAX = 64 * 1024;
const enc = new TextEncoder();

interface FakeHostOptions {
  /** Where the layout persists; null keeps it for the page only. */
  storageKey: string | null;
  /** This Host's home, for `~`. */
  home: string;
  /** Record Resume entries (the local Host only). */
  resume: boolean;
  /** The fake shell's `offline` command (the remote Host only). */
  onOffline?: () => void;
}

interface MockLayout {
  groups: Group[];
  tabs: Record<string, Tab>;
  activeTabId: string | null;
}

/** One fake Host: its Sessions and its layout, as the core would hold them. */
function createFakeHost(opts: FakeHostOptions) {
  const home = opts.home;
  const sessions = new Map<SessionId, FakeSession>();
  const infoCbs = new Set<InfoCb>();
  const exitCbs = new Set<ExitCb>();
  const layoutCbs = new Set<LayoutCb>();
  let nextId = 1;
  let revision = 0;

  function gitFor(cwd: string): GitInfo | null {
    const jack = `${home}/Dev/jack`;
    if (cwd.startsWith(`${jack}/.claude/worktrees/`)) {
      const name = cwd.slice(`${jack}/.claude/worktrees/`.length).split("/")[0];
      return {
        repoName: "jack",
        commonDir: `${jack}/.git`,
        worktreeRoot: `${jack}/.claude/worktrees/${name}`,
        worktreeName: name,
        branch: name === "navbar" ? "navbar-new-gift" : name,
        headShort: null,
      };
    }
    if (cwd === jack || cwd.startsWith(`${jack}/`)) {
      return { repoName: "jack", commonDir: `${jack}/.git`, worktreeRoot: jack, worktreeName: null, branch: "main", headShort: null };
    }
    const det = `${home}/Dev/detached`;
    if (cwd === det || cwd.startsWith(`${det}/`)) {
      return { repoName: "detached", commonDir: `${det}/.git`, worktreeRoot: det, worktreeName: null, branch: null, headShort: "3508290" };
    }
    return null;
  }

  function info(s: FakeSession): SessionInfo {
    return {
      sessionId: s.id,
      foreground: s.fg,
      shellIsForeground: s.fg === "zsh",
      agent: s.agent,
      cwd: s.remote ? home : s.cwd,
      remote: s.remote,
      git: s.remote ? null : gitFor(s.cwd),
      title: s.title,
      bells: s.bells,
      status: s.agent ? s.status : null,
    };
  }

  /** A fake OSC title: written to the Terminal, and kept as the Host would read it. */
  function setTitle(s: FakeSession, title: string) {
    s.title = title;
    out(s, `\x1b]0;${title}\x07`);
  }

  /** The fake agent works for a while, then is done; the status changes go out as the Host's would. */
  function busy(s: FakeSession, ms: number) {
    if (s.busy) clearTimeout(s.busy);
    s.status = "running";
    s.busy = setTimeout(() => {
      s.busy = null;
      if (s.agent && s.status === "running") {
        s.status = "done";
        if (s.agent === "codex") setTitle(s, "jack");
        if (s.agent === "gemini") setTitle(s, "◇ Ready (jack)");
        emit(s);
      }
    }, ms);
  }

  function emit(s: FakeSession) {
    if (opts.resume) recordResume(sessions);
    const i = info(s);
    // As the Host does from the monitor: the Tab's last cwd follows its Session.
    const tab = Object.values(model.tabs).find((t) => t.sessionId === s.id);
    if (tab && !i.remote && i.cwd && tab.lastCwd !== i.cwd) {
      tab.lastCwd = i.cwd;
      layoutChanged();
    }
    setTimeout(() => infoCbs.forEach((cb) => cb(i)), 50);
  }

  function out(s: FakeSession, text: string) {
    const bytes = enc.encode(text);
    s.ring.push(bytes);
    s.ringBytes += bytes.length;
    while (s.ringBytes > RING_MAX && s.ring.length > 1) s.ringBytes -= s.ring.shift()!.length;
    if (s.onData) s.onData(bytes);
    else s.held.push(bytes);
  }

  function prompt(s: FakeSession) {
    const short = s.cwd.startsWith(home) ? "~" + s.cwd.slice(home.length) : s.cwd;
    out(s, s.fg === "zsh" ? `\x1b[36m${short}\x1b[0m %% `.replace("%%", "%") : `${s.fg}> `);
  }

  function run(s: FakeSession, cmd: string) {
    const [head, ...rest] = cmd.trim().split(/\s+/);
    if (s.fg !== "zsh") {
      if (head === "exit") {
        s.fg = "zsh";
        s.agent = null;
        s.remote = false;
        s.command = null;
        s.status = null;
        if (s.busy) clearTimeout(s.busy);
        s.busy = null;
        setTitle(s, "");
        emit(s);
      } else if (head) {
        out(s, `(${s.fg} pretends to work on: ${cmd})\r\n`);
        if (s.agent === "codex") setTitle(s, "⠋ jack");
        if (s.agent === "gemini") setTitle(s, "✦ Working… (jack)");
        if (s.agent === "claude") setTitle(s, `◐ ${cmd.trim()}`);
        if (head === "ask") {
          out(s, "\x07");
          s.bells += 1;
          if (s.busy) clearTimeout(s.busy);
          s.busy = null;
          s.status = "needs-input";
          if (s.agent === "codex") setTitle(s, "[ ! ] Action Required");
          if (s.agent === "gemini") setTitle(s, "✋ Action Required (jack)");
          if (s.agent === "claude") setTitle(s, `✳ ${cmd.trim()}`);
        } else if (s.agent) {
          busy(s, 3000);
        }
        emit(s);
      }
      prompt(s);
      return;
    }
    switch (head) {
      case "":
        break;
      case "cd": {
        const arg = rest[0] ?? "~";
        s.cwd = arg.startsWith("~") ? home + arg.slice(1) : arg.startsWith("/") ? arg : `${s.cwd}/${arg}`;
        s.cwd = s.cwd.replace(/\/+$/, "") || "/";
        emit(s);
        break;
      }
      case "claude":
      case "codex":
      case "gemini":
        s.fg = head;
        s.agent = head;
        s.status = "done";
        out(s, `\x1b[35m[fake ${head}]\x1b[0m type anything; \`ask\` rings the bell; \`exit\` quits\r\n`);
        if (head === "gemini") setTitle(s, "◇ Ready (jack)");
        emit(s);
        break;
      case "npm":
      case "pnpm":
      case "uv":
        s.fg = head;
        s.command = cmd.trim();
        out(s, `\x1b[2m(fake) ${s.command}: listening on http://localhost:3000\x1b[0m\r\n`);
        emit(s);
        break;
      case "ssh":
        s.fg = "ssh";
        s.remote = true;
        out(s, `Connected to ${rest[0] ?? "example.host"} (fake). \`exit\` to disconnect.\r\n`);
        emit(s);
        break;
      case "exit":
        endSession(s.id, 0);
        return;
      case "help":
        out(s, `fake shell: cd, claude, codex, gemini, ssh, npm, pnpm, uv, exit, ls${opts.onOffline ? ", offline" : ""}\r\n`);
        break;
      case "ls":
        out(s, "README.md  src  package.json\r\n");
        break;
      case "offline":
        if (opts.onOffline) {
          out(s, "\x1b[2m(fake) dropping the connection for 6 s\x1b[0m\r\n");
          setTimeout(() => opts.onOffline?.(), 200);
          break;
        }
        out(s, `zsh: command not found: ${head}\r\n`);
        break;
      default:
        out(s, `zsh: command not found: ${head}\r\n`);
    }
    prompt(s);
  }

  /** Spawn a fake Session (the Host does this as it makes a Tab). Its prompt is held until a Terminal attaches. */
  function spawn(cwd: string | null, resumeKey: string): SessionId {
    const s: FakeSession = {
      id: nextId++,
      onData: null,
      held: [],
      ring: [],
      ringBytes: 0,
      line: "",
      cwd: cwd ?? home,
      fg: "zsh",
      agent: null,
      remote: false,
      command: null,
      resumeKey,
      title: null,
      bells: 0,
      status: null,
      busy: null,
    };
    sessions.set(s.id, s);
    setTimeout(() => {
      if (!sessions.has(s.id)) return;
      out(s, "\x1b[2mmock backend: type `help`\x1b[0m\r\n");
      prompt(s);
      emit(s);
    }, 30);
    return s.id;
  }

  /** A Session ended (its shell exited, or its Tab was closed): its Tab goes with it. */
  function endSession(id: SessionId, code: number | null) {
    if (!sessions.delete(id)) return;
    if (opts.resume) recordResume(sessions);
    const tab = Object.values(model.tabs).find((t) => t.sessionId === id);
    if (tab) removeTab(tab.id);
    setTimeout(() => exitCbs.forEach((cb) => cb({ sessionId: id, code })), 0);
  }

  async function attachSession(id: SessionId, onData: (b: Uint8Array) => void): Promise<void> {
    const s = sessions.get(id);
    if (!s) throw new Error(`no Session ${id}`);
    for (const bytes of s.held) onData(bytes);
    s.held = [];
    s.onData = onData;
  }

  /** As the Host protocol's attach: the recent output whole, then live. */
  function attachWithReplay(id: SessionId, onData: (b: Uint8Array) => void): Uint8Array | null {
    const s = sessions.get(id);
    if (!s) return null;
    s.held = [];
    s.onData = onData;
    const replay = new Uint8Array(s.ringBytes);
    let at = 0;
    for (const chunk of s.ring) {
      replay.set(chunk, at);
      at += chunk.length;
    }
    return replay;
  }

  function detachSession(id: SessionId) {
    const s = sessions.get(id);
    if (s) s.onData = null;
  }

  async function writeSession(id: SessionId, data: string): Promise<void> {
    const s = sessions.get(id);
    if (!s) return;
    for (const ch of data) {
      if (ch === "\r") {
        out(s, "\r\n");
        const cmd = s.line;
        s.line = "";
        run(s, cmd);
      } else if (ch === "\x7f") {
        if (s.line) {
          s.line = s.line.slice(0, -1);
          out(s, "\b \b");
        }
      } else if (ch === "\x03") {
        s.line = "";
        out(s, "^C\r\n");
        if (s.command) {
          s.fg = "zsh";
          s.command = null;
          emit(s);
        }
        prompt(s);
      } else if (ch === "\x15") {
        out(s, "\b \b".repeat(s.line.length));
        s.line = "";
      } else if (ch >= " ") {
        s.line += ch;
        out(s, ch);
      }
    }
  }

  async function sessionInfo(id: SessionId): Promise<SessionInfo | null> {
    const s = sessions.get(id);
    return s ? info(s) : null;
  }

  function infos(): SessionInfo[] {
    return [...sessions.values()].map(info);
  }

  /** What `ls` lists: every fake cwd holds these, so its output has paths to double-click. */
  const FAKE_FILES = ["README.md", "src", "package.json"];

  async function resolvePaths(id: SessionId, candidates: string[]): Promise<(string | null)[]> {
    const s = sessions.get(id);
    return candidates.map((c) => {
      if (!s || s.remote) return null;
      const abs = c.startsWith("~/") ? home + c.slice(1) : c.startsWith("/") ? c : `${s.cwd}/${c}`;
      const name = abs.slice(abs.lastIndexOf("/") + 1);
      return FAKE_FILES.includes(name) ? abs : null;
    });
  }

  async function onSessionInfo(cb: InfoCb) {
    infoCbs.add(cb);
    return () => void infoCbs.delete(cb);
  }

  async function onSessionExit(cb: ExitCb) {
    exitCbs.add(cb);
    return () => void exitCbs.delete(cb);
  }

  // --- The layout: the Host's model, in localStorage -------------------------------------------

  const model: MockLayout = loadModel();
  // Launch: every persisted Tab respawns; a fresh install gets one Tab.
  for (const t of Object.values(model.tabs)) t.sessionId = spawn(t.lastCwd, t.id);
  if (Object.keys(model.tabs).length === 0 && typeof localStorage !== "undefined") {
    const group = model.groups[0];
    const tab = makeTab(group.id, null);
    group.tabIds.push(tab.id);
    model.activeTabId = tab.id;
  }

  function makeTab(groupId: string, cwd: string | null): Tab {
    const id = mockId("tab");
    const tab: Tab = { id, groupId, sessionId: spawn(cwd, id), customTitle: null, lastCwd: cwd };
    model.tabs[id] = tab;
    return tab;
  }

  function loadModel(): MockLayout {
    try {
      const raw = JSON.parse((opts.storageKey && localStorage.getItem(opts.storageKey)) || "null") as {
        groups?: Group[];
        tabs?: Omit<Tab, "sessionId">[];
        activeTabId?: string | null;
      } | null;
      if (raw?.groups?.length && raw.tabs) {
        const tabs: Record<string, Tab> = {};
        for (const t of raw.tabs) tabs[t.id] = { ...t, sessionId: null };
        return { groups: raw.groups, tabs, activeTabId: raw.activeTabId ?? null };
      }
    } catch {
      /* start fresh */
    }
    return { groups: [{ id: mockId("group"), name: "Tabs", collapsed: false, tabIds: [] }], tabs: {}, activeTabId: null };
  }

  function snapshot(): LayoutSnapshot {
    return structuredClone({ revision, groups: model.groups, tabs: model.tabs, activeTabId: model.activeTabId });
  }

  /** Every change: a new revision, one `layout` event, the file (minus Session ids). */
  function layoutChanged() {
    revision += 1;
    if (opts.storageKey) {
      try {
        localStorage.setItem(
          opts.storageKey,
          JSON.stringify({
            version: 2,
            groups: model.groups,
            tabs: Object.values(model.tabs).map(({ sessionId: _, ...t }) => t),
            activeTabId: model.activeTabId,
          }),
        );
      } catch {
        /* ignore */
      }
    }
    const snap = snapshot();
    setTimeout(() => layoutCbs.forEach((cb) => cb(snap)), 0);
  }

  function orderedTabIds(): string[] {
    return model.groups.flatMap((g) => g.tabIds);
  }

  function removeTab(tabId: string) {
    const tab = model.tabs[tabId];
    if (!tab) return;
    const order = orderedTabIds();
    const idx = order.indexOf(tabId);
    const next = order[idx + 1] ?? order[idx - 1] ?? null;
    delete model.tabs[tabId];
    const group = model.groups.find((g) => g.id === tab.groupId);
    if (group) group.tabIds = group.tabIds.filter((id) => id !== tabId);
    if (model.activeTabId === tabId) model.activeTabId = next;
    layoutChanged();
  }

  async function layoutGet(): Promise<LayoutSnapshot> {
    return snapshot();
  }

  async function onLayout(cb: LayoutCb) {
    layoutCbs.add(cb);
    return () => void layoutCbs.delete(cb);
  }

  async function tabNew(o: TabNewOptions): Promise<Tab> {
    const active = model.activeTabId ? model.tabs[model.activeTabId] : null;
    const groupId = o.groupId ?? active?.groupId ?? model.groups[0]?.id;
    const group = model.groups.find((g) => g.id === groupId);
    if (!group) throw new Error(`no Group ${groupId}`);
    const after = o.afterTabId ?? (active && active.groupId === groupId ? active.id : null);
    const tab = makeTab(group.id, o.cwd ?? active?.lastCwd ?? null);
    const afterAt = after ? group.tabIds.indexOf(after) : -1;
    group.tabIds.splice(afterAt === -1 ? group.tabIds.length : afterAt + 1, 0, tab.id);
    model.activeTabId = tab.id;
    layoutChanged();
    return structuredClone(tab);
  }

  async function tabClose(tabId: string): Promise<void> {
    const tab = model.tabs[tabId];
    if (!tab) throw new Error(`no Tab ${tabId}`);
    removeTab(tabId);
    if (tab.sessionId !== null) endSession(tab.sessionId, null);
  }

  async function tabRename(tabId: string, title: string): Promise<void> {
    const tab = model.tabs[tabId];
    if (!tab) throw new Error(`no Tab ${tabId}`);
    tab.customTitle = title.trim() === "" ? null : title.trim();
    layoutChanged();
  }

  async function tabMove(tabId: string, groupId: string, index?: number): Promise<void> {
    const tab = model.tabs[tabId];
    const target = model.groups.find((g) => g.id === groupId);
    if (!tab || !target) throw new Error("no such Tab or Group");
    const source = model.groups.find((g) => g.id === tab.groupId);
    let insertAt = index ?? target.tabIds.length;
    if (source) {
      const removedAt = source.tabIds.indexOf(tabId);
      source.tabIds = source.tabIds.filter((id) => id !== tabId);
      if (source === target && removedAt !== -1 && removedAt < insertAt) insertAt -= 1;
    }
    target.tabIds.splice(Math.max(0, Math.min(insertAt, target.tabIds.length)), 0, tabId);
    tab.groupId = groupId;
    layoutChanged();
  }

  async function tabActivate(tabId: string): Promise<void> {
    if (!model.tabs[tabId]) throw new Error(`no Tab ${tabId}`);
    if (model.activeTabId === tabId) return;
    model.activeTabId = tabId;
    layoutChanged();
  }

  async function groupNew(name?: string | null, tabId?: string | null): Promise<Group> {
    const group: Group = { id: mockId("group"), name: name?.trim() || "New Group", collapsed: false, tabIds: [] };
    model.groups.push(group);
    if (tabId) await tabMove(tabId, group.id);
    else layoutChanged();
    return structuredClone(group);
  }

  async function groupRename(groupId: string, name: string): Promise<void> {
    const group = model.groups.find((g) => g.id === groupId);
    if (!group) throw new Error(`no Group ${groupId}`);
    if (name.trim() === "") return;
    group.name = name.trim();
    layoutChanged();
  }

  async function groupMove(groupId: string, index: number): Promise<void> {
    const idx = model.groups.findIndex((g) => g.id === groupId);
    if (idx === -1) throw new Error(`no Group ${groupId}`);
    const [group] = model.groups.splice(idx, 1);
    model.groups.splice(Math.max(0, Math.min(index, model.groups.length)), 0, group);
    layoutChanged();
  }

  async function groupDelete(groupId: string): Promise<void> {
    if (model.groups.length <= 1) throw new Error("the last Group cannot be deleted");
    const group = model.groups.find((g) => g.id === groupId);
    if (!group) throw new Error(`no Group ${groupId}`);
    const tabs = group.tabIds.map((id) => model.tabs[id]).filter((t): t is Tab => Boolean(t));
    model.groups = model.groups.filter((g) => g.id !== groupId);
    for (const t of tabs) delete model.tabs[t.id];
    if (model.activeTabId && tabs.some((t) => t.id === model.activeTabId)) model.activeTabId = orderedTabIds()[0] ?? null;
    layoutChanged();
    for (const t of tabs) if (t.sessionId !== null) endSession(t.sessionId, null);
  }

  async function groupSetCollapsed(groupId: string, collapsed: boolean): Promise<void> {
    const group = model.groups.find((g) => g.id === groupId);
    if (!group) throw new Error(`no Group ${groupId}`);
    if (group.collapsed === collapsed) return;
    group.collapsed = collapsed;
    layoutChanged();
  }

  // --- Activity: invented ------------------------------------------------------------------------

  function activitySnapshot(): ActivitySnapshot {
    const processes: ActivityProcess[] = SYSTEM_PROCESSES.map(([name, cpu, mem], i) => ({
      pid: 100 + i,
      name,
      cpu: jitter(cpu),
      mem,
      sessionId: null,
    }));
    for (const s of sessions.values()) {
      processes.push({ pid: 5000 + s.id * 10, name: "zsh", cpu: 0, mem: 3 * MB, sessionId: s.id });
      if (s.fg !== "zsh") {
        const mem = s.agent ? 3.5 * 1024 * MB : 240 * MB;
        processes.push({ pid: 5001 + s.id * 10, name: s.fg, cpu: jitter(s.agent ? 35 : 5), mem, sessionId: s.id });
      }
    }
    const bySession = new Map<SessionId, ActivitySession>();
    for (const p of processes) {
      if (p.sessionId === null) continue;
      const a = bySession.get(p.sessionId) ?? { sessionId: p.sessionId, cpu: 0, mem: 0, processes: 0 };
      a.cpu += p.cpu;
      a.mem += p.mem;
      a.processes += 1;
      bySession.set(p.sessionId, a);
    }
    return {
      cpuCount: 10,
      cpuTotal: processes.reduce((sum, p) => sum + p.cpu, 0),
      memUsed: 8 * 1024 * MB + [...bySession.values()].reduce((sum, s) => sum + s.mem, 0),
      memWired: 2.5 * 1024 * MB,
      memCompressed: 1.5 * 1024 * MB,
      memTotal: 16 * 1024 * MB,
      sessions: [...bySession.values()],
      processes,
    };
  }

  return {
    sessions,
    attachSession,
    attachWithReplay,
    detachSession,
    writeSession,
    sessionInfo,
    infos,
    resolvePaths,
    onSessionInfo,
    onSessionExit,
    layoutGet,
    onLayout,
    tabNew,
    tabClose,
    tabRename,
    tabMove,
    tabActivate,
    groupNew,
    groupRename,
    groupMove,
    groupDelete,
    groupSetCollapsed,
    activitySnapshot,
    /** For the fake remote Host's seed: a Tab in a Group, by name. */
    seed(groupName: string, cwd: string | null, typed?: string): void {
      let group = model.groups.find((g) => g.name === groupName);
      if (!group) {
        group = { id: mockId("group"), name: groupName, collapsed: false, tabIds: [] };
        model.groups.push(group);
      }
      const tab = makeTab(group.id, cwd);
      group.tabIds.push(tab.id);
      model.activeTabId ??= tab.id;
      if (typed && tab.sessionId !== null) {
        const id = tab.sessionId;
        setTimeout(() => void writeSession(id, `${typed}\r`), 80);
      }
      layoutChanged();
    },
  };
}

function mockId(prefix: string): string {
  return `${prefix}_${Math.random().toString(16).slice(2)}${Math.random().toString(16).slice(2)}`;
}

const MB = 1024 * 1024;
const SYSTEM_PROCESSES: [name: string, cpu: number, mem: number][] = [
  ["WindowServer", 18, 420 * MB],
  ["kernel_task", 6, 12 * MB],
  ["Google Chrome Helper (Renderer)", 9, 610 * MB],
  ["Google Chrome", 3, 380 * MB],
  ["Slack Helper (Renderer)", 2, 290 * MB],
  ["mds_stores", 4, 60 * MB],
  ["Finder", 0.3, 140 * MB],
  ["coreaudiod", 0.8, 24 * MB],
  ["launchd", 0.1, 18 * MB],
  ["com.apple.WebKit.WebContent", 1.2, 210 * MB],
];

function jitter(v: number): number {
  return v * (0.5 + Math.random());
}

// --- The local Host: behind the IPC exports ----------------------------------------------------

const LAYOUT_KEY = "sidebar-term:mock-layout";

const local = createFakeHost({ storageKey: LAYOUT_KEY, home: HOME, resume: true });

export const {
  attachSession,
  writeSession,
  sessionInfo,
  resolvePaths,
  onSessionInfo,
  onSessionExit,
  layoutGet,
  onLayout,
  tabNew,
  tabClose,
  tabRename,
  tabMove,
  tabActivate,
  groupNew,
  groupRename,
  groupMove,
  groupDelete,
  groupSetCollapsed,
} = local;

export async function resizeSession(_id: SessionId, _c: number, _r: number): Promise<void> {}

export async function openPath(path: string): Promise<void> {
  console.info("[mock] open", path);
}

const activityCbs = new Set<ActivityCb>();
let activityTimer: ReturnType<typeof setInterval> | null = null;

export async function watchActivity(on: boolean): Promise<void> {
  if (activityTimer !== null) clearInterval(activityTimer);
  activityTimer = null;
  if (!on) return;
  const tick = () => {
    const snapshot = local.activitySnapshot();
    activityCbs.forEach((cb) => cb(snapshot));
  };
  tick();
  activityTimer = setInterval(tick, 2000);
}

export async function onActivity(cb: ActivityCb) {
  activityCbs.add(cb);
  return () => void activityCbs.delete(cb);
}

type UsageCb = (u: UsageSnapshot) => void;
const usageCbs = new Set<UsageCb>();
let usageTimer: ReturnType<typeof setInterval> | null = null;
const startedAt = Date.now();
const HOUR = 3_600_000;

function fakeUsage(agent: AgentKind): AgentUsage | null {
  const now = Date.now();
  switch (agent) {
    case "claude":
      return {
        agent,
        windows: [
          { label: "5h", usedPercent: Math.min(100, 48 + (now - startedAt) / 20_000), resetsAt: now + 2.2 * HOUR },
          { label: "Week", usedPercent: 83, resetsAt: now + 76 * HOUR },
        ],
        plan: "max",
        updatedAt: now,
        error: null,
      };
    case "codex":
      return {
        agent,
        windows: [
          { label: "5h", usedPercent: 12, resetsAt: now + 0.6 * HOUR },
          { label: "Week", usedPercent: 97, resetsAt: now + 120 * HOUR },
        ],
        plan: "plus",
        updatedAt: now - 3 * HOUR,
        error: null,
      };
    case "gemini":
      return null;
  }
}

export async function watchUsage(on: boolean, agents: AgentKind[]): Promise<void> {
  if (usageTimer !== null) clearInterval(usageTimer);
  usageTimer = null;
  if (!on) return;
  const tick = () => {
    const snapshot = { agents: agents.map(fakeUsage).filter((a): a is AgentUsage => a !== null) };
    usageCbs.forEach((cb) => cb(snapshot));
  };
  tick();
  usageTimer = setInterval(tick, 5000);
}

export async function onUsage(cb: UsageCb) {
  usageCbs.add(cb);
  return () => void usageCbs.delete(cb);
}

let caffeinated = false;

export async function caffeinateState(): Promise<boolean> {
  return caffeinated;
}

export async function setCaffeinate(on: boolean): Promise<boolean> {
  caffeinated = on;
  return caffeinated;
}

// --- Remote: nothing listens; the Settings section can be exercised, pairing shows a code ---

const remote: RemoteSnapshot = {
  on: false,
  port: 47611,
  url: null,
  error: null,
  tailscale: { installed: true, running: true, dnsName: "your-mac.tail1234.ts.net", error: null },
  clients: 0,
  devices: [{ id: "d1", name: "iPhone", createdAt: Date.now() - 86_400_000, lastSeenAt: Date.now() - 3_600_000, login: null }],
  pairing: null,
};
const remoteCbs = new Set<(s: RemoteSnapshot) => void>();

function remoteChanged() {
  const copy = structuredClone(remote);
  for (const cb of remoteCbs) cb(copy);
}

export async function remoteState(): Promise<RemoteSnapshot> {
  return structuredClone(remote);
}

export async function setRemote(on: boolean): Promise<RemoteSnapshot> {
  remote.on = on;
  remote.url = on ? `https://${remote.tailscale.dnsName}/m` : null;
  if (!on) remote.pairing = null;
  remoteChanged();
  return structuredClone(remote);
}

export async function remotePairBegin(): Promise<Pairing> {
  remote.pairing = { code: "ABCD EFGH", url: `${remote.url ?? "http://127.0.0.1:47611/m"}#pair=ABCDEFGH`, expiresAt: Date.now() + 600_000 };
  remoteChanged();
  return structuredClone(remote.pairing);
}

export async function remotePairCancel(): Promise<void> {
  remote.pairing = null;
  remoteChanged();
}

export async function remoteRevoke(id: string): Promise<void> {
  remote.devices = remote.devices.filter((d) => d.id !== id);
  remoteChanged();
}

export async function onRemote(cb: (s: RemoteSnapshot) => void) {
  remoteCbs.add(cb);
  return () => void remoteCbs.delete(cb);
}

// --- Memory Guard: the policy on the invented memory --------------------------------------------

let guard: GuardSnapshot = { on: false, limitPercent: 85, frozen: [] };
let guardVisibleId: SessionId | null = null;
let guardTimer: ReturnType<typeof setInterval> | null = null;
let guardLastStep = 0;
const guardCbs = new Set<(g: GuardSnapshot) => void>();

function guardChanged(): GuardSnapshot {
  const copy = { ...guard, frozen: [...guard.frozen] };
  guardCbs.forEach((cb) => cb(copy));
  return copy;
}

/** guard.rs's policy, minus the clocks' finer points: freeze the heaviest, thaw the oldest. */
function guardTick() {
  const snap = local.activitySnapshot();
  guard.frozen = guard.frozen.filter((f) => local.sessions.has(f.sessionId));
  if (Date.now() - guardLastStep < 10_000) return;
  const used = (snap.memUsed / snap.memTotal) * 100;
  if (used > guard.limitPercent) {
    const frozen = new Set(guard.frozen.map((f) => f.sessionId));
    const pick = snap.sessions
      .filter((s) => s.mem >= 128 * MB && s.sessionId !== guardVisibleId && !frozen.has(s.sessionId))
      .sort((a, b) => b.mem - a.mem)[0];
    if (!pick) return;
    guard.frozen.push({ sessionId: pick.sessionId, mem: pick.mem, frozenAt: Date.now(), manual: false });
  } else if (used < guard.limitPercent - 10 && guard.frozen.some((f) => !f.manual)) {
    guard.frozen.splice(
      guard.frozen.findIndex((f) => !f.manual),
      1,
    );
  } else return;
  guardLastStep = Date.now();
  guardChanged();
}

export async function guardState(): Promise<GuardSnapshot> {
  return { ...guard, frozen: [...guard.frozen] };
}

export async function setGuard(on: boolean, limitPercent: number): Promise<GuardSnapshot> {
  guard = {
    on,
    limitPercent: Math.min(95, Math.max(50, limitPercent)),
    frozen: on ? guard.frozen : guard.frozen.filter((f) => f.manual),
  };
  if (guardTimer !== null) clearInterval(guardTimer);
  guardTimer = on ? setInterval(guardTick, 2000) : null;
  return guardChanged();
}

export async function guardVisible(sessionId: SessionId | null): Promise<void> {
  guardVisibleId = sessionId;
  const before = guard.frozen.length;
  guard.frozen = guard.frozen.filter((f) => f.sessionId !== sessionId);
  if (guard.frozen.length !== before) {
    guardLastStep = Date.now();
    guardChanged();
  }
}

export async function guardFreeze(sessionId: SessionId): Promise<GuardSnapshot> {
  if (sessionId === guardVisibleId) throw new Error("the Tab in view cannot be frozen");
  if (!guard.frozen.some((f) => f.sessionId === sessionId)) {
    const mem = local.activitySnapshot().sessions.find((s) => s.sessionId === sessionId)?.mem ?? 0;
    guard.frozen.push({ sessionId, mem, frozenAt: Date.now(), manual: true });
  }
  return guardChanged();
}

export async function guardThaw(sessionId: SessionId): Promise<GuardSnapshot> {
  guard.frozen = guard.frozen.filter((f) => f.sessionId !== sessionId);
  return guardChanged();
}

export async function onGuard(cb: (g: GuardSnapshot) => void) {
  guardCbs.add(cb);
  return () => void guardCbs.delete(cb);
}

// --- Settings: localStorage; a fake paired Host is seeded until the user removes it -------------

const SETTINGS_KEY = "sidebar-term:mock-settings";
const MOCK_HOST_URL = "mock://dell";

export async function loadSettings(): Promise<unknown | null> {
  let saved: Record<string, unknown> = {};
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) saved = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    saved = {};
  }
  if (!("hosts" in saved)) {
    saved.hosts = [{ id: "h_mockdell", url: MOCK_HOST_URL, token: "mock-token", name: "dell" }];
  }
  return saved;
}

export async function saveSettings(settings: unknown): Promise<void> {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    /* ignore */
  }
}

// --- Resume: localStorage stands in for resume.json ---------------------------------------------

const RESUME_KEY = "sidebar-term:mock-resume";

interface MockResume {
  running: ResumeEntry[];
  leftover: ResumeEntry[];
}

function readResume(): MockResume {
  try {
    const raw = JSON.parse(localStorage.getItem(RESUME_KEY) ?? "null");
    return { running: raw?.running ?? [], leftover: raw?.leftover ?? [] };
  } catch {
    return { running: [], leftover: [] };
  }
}

function writeResume(r: MockResume): void {
  try {
    localStorage.setItem(RESUME_KEY, JSON.stringify(r));
  } catch {
    /* ignore */
  }
}

// Loading this module is a launch: what the last page was running becomes leftover.
if (typeof localStorage !== "undefined") {
  const r = readResume();
  const keys = new Set(r.running.map((e) => e.key));
  writeResume({ running: [], leftover: [...r.leftover.filter((e) => !keys.has(e.key)), ...r.running] });
}

function recordResume(sessions: Map<SessionId, FakeSession>): void {
  const running: ResumeEntry[] = [];
  for (const s of sessions.values()) {
    if (!s.resumeKey) continue;
    if (s.agent === "claude") {
      running.push({ key: s.resumeKey, kind: "claude", line: "claude --resume 5b6d103b-fake", cwd: s.cwd });
    } else if (s.command) {
      running.push({ key: s.resumeKey, kind: "command", line: s.command, cwd: s.cwd });
    }
  }
  writeResume({ ...readResume(), running });
}

export async function resumeLeftover(): Promise<ResumeEntry[]> {
  return readResume().leftover;
}

export async function resumeForget(keys: string[]): Promise<void> {
  const r = readResume();
  writeResume({ ...r, leftover: r.leftover.filter((e) => !keys.includes(e.key)) });
}

// --- A fake paired Host, "dell": a second fake Host behind a fake Host protocol connection ------

const DELL_HOME = "/home/you";
let dell: ReturnType<typeof createFakeHost> | null = null;
/** The live fake connection's outage hook (the fake shell's `offline`). */
let dellOutage: (() => void) | null = null;

function dellHost() {
  if (dell) return dell;
  const key = "sidebar-term:mock-host-layout";
  let fresh = true;
  try {
    fresh = localStorage.getItem(key) === null;
  } catch {
    /* no storage: seed every time */
  }
  dell = createFakeHost({ storageKey: key, home: DELL_HOME, resume: false, onOffline: () => dellOutage?.() });
  if (fresh) {
    // A first run: a running fake agent, a Worktree, and a server, in two Groups.
    dell.seed("Agents", `${DELL_HOME}/Dev/jack`, "claude");
    dell.seed("Agents", `${DELL_HOME}/Dev/jack/.claude/worktrees/navbar`);
    dell.seed("Servers", `${DELL_HOME}/Dev/jack`, "npm run dev");
  }
  return dell;
}

/** Pair with a fake Host: any 8-character code is right. */
export async function pairHost(url: string, code: string, name: string): Promise<Paired> {
  if (url !== MOCK_HOST_URL) throw new Error(`Could not reach ${url}: the mock knows only ${MOCK_HOST_URL}.`);
  if (code.replace(/[^A-Za-z0-9]/g, "").length !== 8) throw new Error("Wrong code. 4 tries left.");
  return { token: "mock-token", device: name };
}

/** A fake Host protocol connection to "dell": hello after a moment, then its layout and Sessions live. */
export function hostClient(url: string, token: string): HostClient {
  const listeners: { [K in keyof ClientEvents]: Set<ClientEvents[K]> } = {
    status: new Set(),
    hello: new Set(),
    layout: new Set(),
    session: new Set(),
    activity: new Set(),
    attached: new Set(),
    resized: new Set(),
    output: new Set(),
    exit: new Set(),
    error: new Set(),
    unauthorized: new Set(),
  };
  function emit<K extends keyof ClientEvents>(event: K, ...args: Parameters<ClientEvents[K]>) {
    for (const cb of listeners[event]) (cb as (...a: Parameters<ClientEvents[K]>) => void)(...args);
  }
  const wanted = new Set<SessionId>();
  let online = false;
  let closed = false;
  let offs: (() => void)[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;
  let activityTimer: ReturnType<typeof setInterval> | null = null;

  function attachNow(host: ReturnType<typeof createFakeHost>, id: SessionId) {
    const replay = host.attachWithReplay(id, (bytes) => emit("output", id, bytes));
    if (!replay) {
      wanted.delete(id);
      emit("exit", id);
      return;
    }
    emit("attached", id, 80, 24);
    if (replay.length) emit("output", id, replay);
  }

  function goOffline(detail: string | null, retryMs: number | null) {
    if (!online) return;
    online = false;
    for (const off of offs) off();
    offs = [];
    if (activityTimer !== null) clearInterval(activityTimer);
    activityTimer = null;
    if (dellOutage === outage) dellOutage = null;
    emit("status", "offline", detail);
    if (retryMs !== null && !closed) timer = setTimeout(connect, retryMs);
  }

  const outage = () => goOffline(null, 6000);

  function connect() {
    if (closed || online) return;
    if (url !== MOCK_HOST_URL) {
      emit("status", "offline", `The mock knows only ${MOCK_HOST_URL}.`);
      return;
    }
    if (token !== "mock-token") {
      emit("status", "offline", "No longer paired with this Host.");
      emit("unauthorized");
      return;
    }
    emit("status", "connecting", null);
    timer = setTimeout(() => {
      timer = null;
      if (closed) return;
      const host = dellHost();
      online = true;
      dellOutage = outage;
      const info: HostInfo = { name: "dell", version: "0.1.0-mock", home: DELL_HOME };
      void host.onLayout((snap) => emit("layout", snap)).then((off) => offs.push(off));
      void host.onSessionInfo((i) => emit("session", i)).then((off) => offs.push(off));
      void host
        .onSessionExit((e) => {
          if (wanted.delete(e.sessionId)) emit("exit", e.sessionId);
        })
        .then((off) => offs.push(off));
      emit("status", "online", null);
      void host.layoutGet().then((layout) => {
        emit("hello", info, "Mac app", layout, host.infos());
        for (const id of wanted) attachNow(host, id);
      });
      activityTimer = setInterval(() => emit("activity", host.activitySnapshot().sessions), 2000);
    }, 400);
  }

  return {
    on(event, cb) {
      listeners[event].add(cb);
      return () => void listeners[event].delete(cb);
    },
    connect,
    reconnectNow() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      connect();
    },
    close() {
      closed = true;
      if (timer !== null) clearTimeout(timer);
      timer = null;
      goOffline(null, null);
    },
    attach(id) {
      wanted.add(id);
      if (online) attachNow(dellHost(), id);
    },
    detach(id) {
      wanted.delete(id);
      if (online) dellHost().detachSession(id);
    },
    input(id, data) {
      if (online) void dellHost().writeSession(id, data);
    },
    async command(msg: Command): Promise<CommandResult> {
      if (!online) throw new Error("Not connected.");
      const host = dellHost();
      switch (msg.t) {
        case "resize":
          return undefined;
        case "tab_new":
          return host.tabNew({ groupId: msg.groupId, afterTabId: msg.afterTabId, cwd: msg.cwd, cols: msg.cols, rows: msg.rows });
        case "tab_close":
          return void (await host.tabClose(msg.tabId));
        case "tab_rename":
          return void (await host.tabRename(msg.tabId, msg.title));
        case "tab_move":
          return void (await host.tabMove(msg.tabId, msg.groupId, msg.index));
        case "tab_activate":
          return void (await host.tabActivate(msg.tabId));
        case "group_new":
          return host.groupNew(msg.name, msg.tabId);
        case "group_rename":
          return void (await host.groupRename(msg.groupId, msg.name));
        case "group_move":
          return void (await host.groupMove(msg.groupId, msg.index));
        case "group_delete":
          return void (await host.groupDelete(msg.groupId));
        case "group_set_collapsed":
          return void (await host.groupSetCollapsed(msg.groupId, msg.collapsed));
      }
    },
    async upload(file) {
      if (!online) throw new Error("Not connected.");
      return `${DELL_HOME}/.local/share/sidebar-term/uploads/${Date.now()}/${file.name}`;
    },
  };
}
