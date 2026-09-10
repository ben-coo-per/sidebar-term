// Browser-only fake backend so the UI runs under plain `vite dev` without Rust.
// Each fake Session is a tiny line-echo "shell". Typing one of these commands changes what
// the fake monitor reports, so every sidebar state can be exercised by hand:
//   claude | codex | gemini   -> Agent session (type `exit` to return to the shell)
//   ssh                       -> remote hop
//   cd <path>                 -> cwd; paths under the fake repos below get git info
//   cd ~/Dev/jack             -> main Worktree on `main`
//   cd ~/Dev/jack/.claude/worktrees/navbar -> linked Worktree `navbar` on `navbar-new-gift`
//   cd ~/Dev/detached         -> detached HEAD
// Layout persistence uses localStorage.

import type { SpawnOptions } from "./ipc";
import type { AgentKind, GitInfo, SessionExit, SessionId, SessionInfo } from "./types";

type InfoCb = (i: SessionInfo) => void;
type ExitCb = (e: SessionExit) => void;

interface FakeSession {
  id: SessionId;
  onData: (b: Uint8Array) => void;
  line: string;
  cwd: string;
  fg: string;
  agent: AgentKind | null;
  remote: boolean;
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
      out(s, "\x1b]0;\x07");
      emit(s);
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
    case "ssh":
      s.fg = "ssh";
      s.remote = true;
      out(s, `Connected to ${rest[0] ?? "example.host"} (fake). \`exit\` to disconnect.\r\n`);
      emit(s);
      break;
    case "exit":
      sessions.delete(s.id);
      exitCbs.forEach((cb) => cb({ sessionId: s.id, code: 0 }));
      return;
    case "help":
      out(s, "fake shell: cd, claude, codex, gemini, ssh, exit, ls\r\n");
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
      prompt(s);
    } else if (ch >= " ") {
      s.line += ch;
      out(s, ch);
    }
  }
}

export async function resizeSession(_id: SessionId, _c: number, _r: number): Promise<void> {}

export async function killSession(id: SessionId): Promise<void> {
  if (sessions.delete(id)) exitCbs.forEach((cb) => cb({ sessionId: id, code: null }));
}

export async function sessionInfo(id: SessionId): Promise<SessionInfo | null> {
  const s = sessions.get(id);
  return s ? info(s) : null;
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
