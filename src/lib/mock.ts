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
// Layout persistence uses localStorage. Activity is invented: each fake Session has a shell (and
// its foreground program, busy when it is an agent) next to a fixed cast of jittering system
// processes. Usage is invented too: fixed limits, Claude Code's 5-hour window creeping up.
// Caffeinate is only a flag: nothing is kept awake. Memory Guard runs its policy on the invented
// memory (each running agent adds 3.5 GB of 16 GB): open two agent Tabs to see one frozen, and
// `exit` one to see it thawed. Frozen Sessions are only marked, nothing stops. Resume is kept in localStorage like Rust's
// resume.json: start `claude` or `npm run dev` in a Tab, reload the page, and the banner offers it.
// Suites are invented: in a shell or under a fake agent, these start a fake test run that shows
// each of the Tab's states (Ctrl-C or `exit` ends one early):
//   pnpm test | vitest      -> progress known: 48 tests over 24 s, two fail (state 1)
//   jest                    -> tests counted, no total
//   pytest                  -> history only: usually ~40 s, this run takes 50 s, so it overruns (state 2)
//   cargo test | go test    -> nothing known: 4 s of building, then 12 s of testing (state 3)

import type { SpawnOptions } from "./ipc";
import type {
  ActivityProcess,
  ActivitySession,
  ActivitySnapshot,
  AgentKind,
  AgentUsage,
  GitInfo,
  GuardSnapshot,
  ResumeEntry,
  SessionExit,
  SessionId,
  SessionInfo,
  SuiteSnapshot,
  UsageSnapshot,
} from "./types";

type InfoCb = (i: SessionInfo) => void;
type ExitCb = (e: SessionExit) => void;
type ActivityCb = (a: ActivitySnapshot) => void;

interface FakeSession {
  id: SessionId;
  onData: (b: Uint8Array) => void;
  line: string;
  cwd: string;
  fg: string;
  agent: AgentKind | null;
  remote: boolean;
  /** The command line of a fake long-running command, while one runs. */
  command: string | null;
  resumeKey: string | null;
  /** The fake Suite running in this Session, if any. */
  suiteId: number | null;
}

const HOME = "/Users/you";
const enc = new TextEncoder();
const sessions = new Map<SessionId, FakeSession>();
const infoCbs = new Set<InfoCb>();
const exitCbs = new Set<ExitCb>();
let nextId = 1;

function gitFor(cwd: string): GitInfo | null {
  const jack = `${HOME}/Dev/jack`;
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
  const det = `${HOME}/Dev/detached`;
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
    cwd: s.remote ? HOME : s.cwd,
    remote: s.remote,
    git: s.remote ? null : gitFor(s.cwd),
  };
}

function emit(s: FakeSession) {
  recordResume();
  const i = info(s);
  setTimeout(() => infoCbs.forEach((cb) => cb(i)), 50);
}

function out(s: FakeSession, text: string) {
  s.onData(enc.encode(text));
}

function prompt(s: FakeSession) {
  const short = s.cwd.startsWith(HOME) ? "~" + s.cwd.slice(HOME.length) : s.cwd;
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
      endFakeSuite(s);
      out(s, "\x1b]0;\x07");
      emit(s);
    } else if (s.agent && fakeRunner(cmd)) {
      out(s, `(${s.fg} runs \`${cmd.trim()}\` in a Bash tool call)\r\n`);
      startFakeSuite(s, cmd);
    } else if (head) {
      out(s, `(${s.fg} pretends to work on: ${cmd})\r\n`);
      if (s.agent === "codex") out(s, "\x1b]0;⠋ jack\x07");
      if (s.agent === "gemini") out(s, "\x1b]0;✦ Working… (jack)\x07");
      if (head === "ask") {
        out(s, "\x07");
        if (s.agent === "codex") out(s, "\x1b]0;[ ! ] Action Required\x07");
        if (s.agent === "gemini") out(s, "\x1b]0;✋ Action Required (jack)\x07");
      }
    }
    prompt(s);
    return;
  }
  if (fakeRunner(cmd)) {
    s.fg = head;
    s.command = cmd.trim();
    out(s, `\x1b[2m(fake) ${s.command}: running…\x1b[0m\r\n`);
    startFakeSuite(s, cmd);
    emit(s);
    return;
  }
  switch (head) {
    case "":
      break;
    case "cd": {
      const arg = rest[0] ?? "~";
      s.cwd = arg.startsWith("~") ? HOME + arg.slice(1) : arg.startsWith("/") ? arg : `${s.cwd}/${arg}`;
      s.cwd = s.cwd.replace(/\/+$/, "") || "/";
      emit(s);
      break;
    }
    case "claude":
    case "codex":
    case "gemini":
      s.fg = head;
      s.agent = head;
      out(s, `\x1b[35m[fake ${head}]\x1b[0m type anything; \`ask\` rings the bell; \`exit\` quits\r\n`);
      if (head === "gemini") out(s, "\x1b]0;◇ Ready (jack)\x07");
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
      sessions.delete(s.id);
      endFakeSuite(s);
      recordResume();
      exitCbs.forEach((cb) => cb({ sessionId: s.id, code: 0 }));
      return;
    case "help":
      out(s, "fake shell: cd, claude, codex, gemini, ssh, npm, pnpm, uv, exit, ls\r\n");
      out(s, "fake test runs: pnpm test, vitest, jest, pytest, cargo test, go test (in the shell or under an agent)\r\n");
      break;
    case "ls":
      out(s, "README.md  src  package.json\r\n");
      break;
    default:
      out(s, `zsh: command not found: ${head}\r\n`);
  }
  prompt(s);
}

export async function spawnSession(opts: SpawnOptions): Promise<SessionId> {
  const s: FakeSession = {
    id: nextId++,
    onData: opts.onData,
    line: "",
    cwd: opts.cwd ?? HOME,
    fg: "zsh",
    agent: null,
    remote: false,
    command: null,
    resumeKey: opts.resumeKey ?? null,
    suiteId: null,
  };
  sessions.set(s.id, s);
  setTimeout(() => {
    out(s, "\x1b[2mmock backend: type `help`\x1b[0m\r\n");
    prompt(s);
    emit(s);
  }, 30);
  return s.id;
}

export async function writeSession(id: SessionId, data: string): Promise<void> {
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
      endFakeSuite(s);
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

export async function resizeSession(_id: SessionId, _c: number, _r: number): Promise<void> {}

export async function killSession(id: SessionId): Promise<void> {
  const s = sessions.get(id);
  if (!s || !sessions.delete(id)) return;
  if (s.suiteId !== null) fakeSuites.delete(s.suiteId);
  recordResume();
  exitCbs.forEach((cb) => cb({ sessionId: id, code: null }));
}

// --- Suites: fake test runs, one SuiteSnapshot each, ticked every 500 ms like Rust's ------------

type SuiteCb = (s: SuiteSnapshot[]) => void;
const suiteCbs = new Set<SuiteCb>();
let suiteTimer: ReturnType<typeof setInterval> | null = null;
let nextSuiteId = 1;

interface FakeSuite {
  snap: SuiteSnapshot;
  /** How the run plays out. */
  kind: "stream" | "count" | "history" | "none";
  buildMs: number;
  durationMs: number;
  total: number;
  /** Which tests (1-based) fail. */
  failAt: number[];
  endedAt: number | null;
}

const fakeSuites = new Map<number, FakeSuite>();

/** The runner a typed command starts, if it is a fake test run. */
function fakeRunner(cmd: string): string | null {
  const words = cmd.trim().split(/\s+/);
  const [a, b] = words;
  if ((a === "pnpm" || a === "npm") && b === "test") return "vitest";
  if (a === "vitest" || a === "jest" || a === "pytest") return a;
  if ((a === "cargo" || a === "go") && b === "test") return a;
  return null;
}

function startFakeSuite(s: FakeSession, cmd: string) {
  endFakeSuite(s);
  const runner = fakeRunner(cmd) ?? "vitest";
  const now = Date.now();
  const base: SuiteSnapshot = {
    id: nextSuiteId++,
    sessionId: s.id,
    runner,
    phase: "testing",
    startedAt: now,
    elapsedMs: 0,
    done: 0,
    total: null,
    failed: 0,
    outcome: null,
    etaMs: null,
    source: "none",
    typicalMs: null,
    longestMs: null,
    runs: 0,
  };
  let suite: FakeSuite;
  switch (runner) {
    case "vitest":
      suite = { snap: { ...base, source: "stream", total: 48 }, kind: "stream", buildMs: 0, durationMs: 24_000, total: 48, failAt: [7, 19], endedAt: null };
      break;
    case "jest":
      suite = { snap: { ...base, source: "stream" }, kind: "count", buildMs: 0, durationMs: 15_000, total: 30, failAt: [], endedAt: null };
      break;
    case "pytest":
      suite = {
        snap: { ...base, source: "history", typicalMs: 40_000, longestMs: 45_000, runs: 5, etaMs: 40_000 },
        kind: "history",
        buildMs: 0,
        durationMs: 50_000,
        total: 0,
        failAt: [],
        endedAt: null,
      };
      break;
    default:
      suite = { snap: { ...base, phase: "building" }, kind: "none", buildMs: 4_000, durationMs: 16_000, total: 0, failAt: [], endedAt: null };
  }
  fakeSuites.set(suite.snap.id, suite);
  s.suiteId = suite.snap.id;
  if (suiteTimer === null) suiteTimer = setInterval(suiteTick, 500);
  suiteTick();
}

/** The run's process is gone: the Suite ends now and lingers 5 s. */
function endFakeSuite(s: FakeSession) {
  if (s.suiteId === null) return;
  const f = fakeSuites.get(s.suiteId);
  s.suiteId = null;
  if (f && f.endedAt === null) finishFakeSuite(f, Date.now());
}

function finishFakeSuite(f: FakeSuite, now: number) {
  f.endedAt = now;
  f.snap = { ...f.snap, phase: "done", etaMs: null };
  if (f.kind === "stream" || f.kind === "count") {
    f.snap.outcome = f.snap.failed > 0 ? "failed" : "passed";
  }
  const session = [...sessions.values()].find((s) => s.suiteId === f.snap.id);
  if (session) {
    session.suiteId = null;
    if (session.command) {
      session.fg = "zsh";
      session.command = null;
      out(session, `\x1b[2m(fake) ${f.snap.runner}: ${f.snap.outcome ?? "done"}\x1b[0m\r\n`);
      prompt(session);
      emit(session);
    }
  }
}

function suiteTick() {
  const now = Date.now();
  for (const [id, f] of fakeSuites) {
    if (f.endedAt !== null) {
      if (now - f.endedAt > 5_000) fakeSuites.delete(id);
      continue;
    }
    const elapsed = now - f.snap.startedAt;
    f.snap = { ...f.snap, elapsedMs: elapsed };
    if (f.kind === "stream" || f.kind === "count") {
      const done = Math.min(f.total, Math.floor((elapsed / f.durationMs) * f.total));
      f.snap.done = done;
      f.snap.failed = f.failAt.filter((n) => n <= done).length;
      f.snap.etaMs = done > 0 ? Math.round((elapsed * (f.total - done)) / done) : null;
    } else if (f.kind === "history") {
      f.snap.etaMs = Math.max(0, (f.snap.typicalMs ?? 0) - elapsed);
    } else {
      f.snap.phase = elapsed < f.buildMs ? "building" : "testing";
    }
    if (elapsed >= f.durationMs) finishFakeSuite(f, now);
  }
  const list = [...fakeSuites.values()].map((f) => f.snap);
  suiteCbs.forEach((cb) => cb(list));
  if (fakeSuites.size === 0 && suiteTimer !== null) {
    clearInterval(suiteTimer);
    suiteTimer = null;
  }
}

export async function onSuite(cb: SuiteCb) {
  suiteCbs.add(cb);
  return () => void suiteCbs.delete(cb);
}

export async function sessionInfo(id: SessionId): Promise<SessionInfo | null> {
  const s = sessions.get(id);
  return s ? info(s) : null;
}

/** What `ls` lists: every fake cwd holds these, so its output has paths to double-click. */
const FAKE_FILES = ["README.md", "src", "package.json"];

export async function resolvePaths(id: SessionId, candidates: string[]): Promise<(string | null)[]> {
  const s = sessions.get(id);
  return candidates.map((c) => {
    if (!s || s.remote) return null;
    const abs = c.startsWith("~/") ? HOME + c.slice(1) : c.startsWith("/") ? c : `${s.cwd}/${c}`;
    const name = abs.slice(abs.lastIndexOf("/") + 1);
    return FAKE_FILES.includes(name) ? abs : null;
  });
}

export async function openPath(path: string): Promise<void> {
  console.info("[mock] open", path);
}

export async function onSessionInfo(cb: InfoCb) {
  infoCbs.add(cb);
  return () => void infoCbs.delete(cb);
}

export async function onSessionExit(cb: ExitCb) {
  exitCbs.add(cb);
  return () => void exitCbs.delete(cb);
}

const KEY = "sidebar-term:mock-layout";
export async function loadLayout(): Promise<unknown | null> {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

export async function saveLayout(layout: unknown): Promise<void> {
  try {
    localStorage.setItem(KEY, JSON.stringify(layout));
  } catch {
    /* ignore */
  }
}

const activityCbs = new Set<ActivityCb>();
let activityTimer: ReturnType<typeof setInterval> | null = null;
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

export async function watchActivity(on: boolean): Promise<void> {
  if (activityTimer !== null) clearInterval(activityTimer);
  activityTimer = null;
  if (!on) return;
  const tick = () => {
    const snapshot = activitySnapshot();
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
  const snap = activitySnapshot();
  guard.frozen = guard.frozen.filter((f) => sessions.has(f.sessionId));
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
    const mem = activitySnapshot().sessions.find((s) => s.sessionId === sessionId)?.mem ?? 0;
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

const SETTINGS_KEY = "sidebar-term:mock-settings";
export async function loadSettings(): Promise<unknown | null> {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
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

function recordResume(): void {
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
