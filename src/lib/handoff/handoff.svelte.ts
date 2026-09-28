// Handoff (docs/architecture.md "Handoff", CONTEXT.md): "New Tab on <Host>" and "Move Tab to
// <Host>", from a local Tab's context menu and as Hotkeys. A Move reruns what the Tab runs on
// the Host at the mapped checkout (src/lib/handoff/model.ts holds the rules; the Resume rule
// for what to type is reused), and takes a Claude Code conversation's transcript with it.
//
// Order of a Move, so the Session is never lost by accident: probe the local Session (its
// Resume entry, its conversation's files, its checkout's git status), ask the Host whether the
// checkout exists, confirm, create the Tab on the Host (its ack is the point of no return),
// close the local Tab, then ship the conversation and type into the new Tab. Anything failing
// before the ack leaves the local Tab as it was and says why; the conversation's Mac copy is
// deleted only once the Host confirms it has it, and a failed transfer falls back to a fresh
// `claude` with a handoff note.

import { handoffConversationForget, handoffConversationRead, handoffProbe, writeSession } from "../ipc";
import { activateTab, closeTab, layout, newTab, setTabUnread, type Tab } from "../layout.svelte";
import { isLocal, LOCAL_HOST, sessionKey, type HostId } from "../host/ids";
import { hostInput, hostName, hostPathExists, hostPutConversation, hosts, hostState, type HostState } from "../host/hosts.svelte";
import { expandHome, repoPathOnHost } from "../host/settings";
import { sessionState, tabTitle } from "../sessions.svelte";
import { requestChoice, requestNotice } from "../sidebar/confirm.svelte";
import type { HandoffProbe, PathExists } from "../types";
import { handoffInput, handoffNote, landing, moveDetail, pushTyped, type Landing } from "./model";

/** How long to wait for the new Tab's shell to report itself before typing anyway. */
const SHELL_WAIT_MS = 4000;

/** The paired Hosts a Tab can go to: every one that is online. */
export function handoffHosts(): HostState[] {
  return hosts.list.filter((h) => h.paired && h.status === "online");
}

/** The Host a Hotkey targets: the first online paired Host, in Settings order. */
export function defaultHandoffHost(): HostId | null {
  return handoffHosts()[0]?.id ?? null;
}

/** Whether a Tab can be handed off: a local Tab with a Session. */
export function canHandOff(tab: Tab | null): tab is Tab {
  return tab !== null && isLocal(tab.host) && tab.sessionId !== null;
}

function explain(message: string, detail: string): Promise<void> {
  return requestNotice({ message, detail });
}

/** An error's message as one sentence, without its own final full stop. */
function reasonOf(e: unknown): string {
  return (e instanceof Error ? e.message : String(e)).trim().replace(/\.+$/, "");
}

/** The Host's home, from its `hello`; null before the first. */
function homeOf(host: HostId): string | null {
  return hostState(host)?.info?.home ?? null;
}

/** Where a Tab in `repo` lands on `host`, asking the Host what exists. */
async function findLanding(host: HostId, repo: string | null): Promise<{ landing: Landing; mapped: boolean }> {
  const state = hostState(host);
  if (!state) throw new Error("This Host is not paired any more.");
  const home = homeOf(host);
  const repoPath = repoPathOnHost(state, repo, home);
  const root = state.checkoutRoot ? expandHome(state.checkoutRoot, home) : null;
  let repoExists: PathExists = { exists: false, dir: false };
  let rootExists: PathExists = { exists: false, dir: false };
  if (repoPath) repoExists = await hostPathExists(host, repoPath);
  if (repoPath && !repoExists.dir && root) rootExists = await hostPathExists(host, root);
  return {
    landing: landing({ repoPath, repoExists: repoExists.dir, root, rootExists: rootExists.dir, home }),
    mapped: repoPath !== null,
  };
}

/**
 * Wait until the Host has reported the new Session's facts (its shell is up and at its prompt),
 * or `SHELL_WAIT_MS` have passed: what is typed then queues in the pty either way.
 */
function waitForShell(host: HostId, sessionId: number): Promise<void> {
  const key = sessionKey(host, sessionId);
  return new Promise((resolve) => {
    const start = Date.now();
    const tick = () => {
      const info = sessionState(key)?.info;
      if ((info && info.shellIsForeground) || Date.now() - start > SHELL_WAIT_MS) resolve();
      else setTimeout(tick, 100);
    };
    tick();
  });
}

/** Type into the new Tab on the Host once its shell is there. */
async function typeInto(host: HostId, sessionId: number, input: string): Promise<void> {
  if (input === "") return;
  await waitForShell(host, sessionId);
  hostInput(host, sessionId, input);
}

/**
 * A moved Tab that was not in view leaves the view where it is: the new Tab on the Host is
 * marked Unread instead, so it is bold until the user goes to it (a Tab that was in view, and a
 * New Tab on a Host, are shown at once, as `newTab` does).
 */
function markUnread(host: HostId, tabId: string): void {
  // This Mac's snapshot may not have named the Tab yet: mark it as soon as it has.
  const mark = () => {
    if (layout.tabs[tabId]) setTabUnread(tabId, true);
    else if (hostState(host)?.status === "online") setTimeout(mark, 100);
  };
  mark();
}

// ---------------------------------------------------------------------------------------------
// New Tab on <Host>
// ---------------------------------------------------------------------------------------------

/**
 * A new Tab on `host`, in `groupId` there (default: the Host's active Tab's Group) and right
 * after `tab` here, at the Host's checkout of the repo `tab` is in (else its home). Without the
 * checkout, `git clone` is typed there, not run. Shown at once.
 */
export async function newTabOnHost(tab: Tab, host: HostId, groupId?: string): Promise<void> {
  const name = hostName(host);
  try {
    const session = tab.sessionId !== null ? sessionState(sessionKey(tab.host, tab.sessionId)) : null;
    const git = session?.info?.remote ? null : (session?.info?.git ?? null);
    const repo = git?.repoName ?? null;
    const { landing } = await findLanding(host, repo);
    const cloneUrl = landing.kind === "clone" && isLocal(tab.host) && tab.sessionId !== null ? await originOf(tab.sessionId) : null;
    const tabId = await newTab({ host, hostGroupId: groupId, after: tab.id, cwd: landing.cwd });
    if (landing.kind === "clone") {
      // The layout may not have named the new Tab's Session yet.
      const sessionId = await sessionOfTab(tabId);
      if (sessionId === null) throw new Error(`The new Tab on ${name} has no Session; git clone was not typed.`);
      await typeInto(host, sessionId, handoffInput({ landing, entry: null, localRoot: null, branch: null, cloneUrl, conversationMoved: false, note: "" }));
    }
  } catch (e) {
    await explain(`Could not open a Tab on ${name}.`, e instanceof Error ? e.message : String(e));
  }
}

/** `origin`'s URL of a local Session's checkout, for a clone typed on the Host. */
async function originOf(sessionId: number): Promise<string | null> {
  const probe = await handoffProbe(sessionId).catch(() => null);
  return probe?.git?.remoteUrl ?? null;
}

// ---------------------------------------------------------------------------------------------
// Move Tab to <Host>
// ---------------------------------------------------------------------------------------------

/**
 * Hand `tab` off to `host`: its Session's Resume entry is rerun in a new Tab there, in
 * `groupId` (default: the Host's active Tab's Group), at the mapped checkout; here the new Tab
 * takes the local Tab's place. A Claude Code conversation moves with it. The local Tab is
 * closed only after the Host has the new one.
 */
export async function moveTabToHost(tab: Tab, host: HostId, groupId?: string): Promise<void> {
  const name = hostName(host);
  if (!canHandOff(tab) || tab.sessionId === null) return;
  const title = tabTitle(tab);
  const wasInView = layout.activeTabId === tab.id;
  let probe: HandoffProbe | null;
  let landed: { landing: Landing; mapped: boolean };
  try {
    probe = await handoffProbe(tab.sessionId);
    if (!probe) throw new Error("Its Session is gone.");
    const git = probe.info.remote ? null : probe.info.git;
    landed = await findLanding(host, git?.repoName ?? null);
  } catch (e) {
    await explain(`Could not move "${title}" to ${name}.`, e instanceof Error ? e.message : String(e));
    return;
  }
  const { landing, mapped } = landed;
  const git = probe.info.remote ? null : probe.info.git;
  const branch = git?.branch ?? null;
  const detail = moveDetail({ hostName: name, landing, entry: probe.entry, branch, git: probe.git, mapped, inRepo: git !== null });
  const unpushed = probe.git !== null && (probe.git.changes > 0 || probe.git.ahead > 0 || (probe.git.branch !== null && probe.git.upstream === null));
  const choice = await requestChoice({
    message: `Move "${title}" to ${name}?`,
    detail: detail.join("\n"),
    confirmLabel: "Move Tab",
    alternativeLabel: unpushed ? "Type git push first" : undefined,
  });
  if (choice === "cancel") return;
  if (choice === "alternative") {
    await typePushFirst(tab, probe);
    return;
  }

  // The point of no return is the Host's ack of the new Tab.
  let newTabId: string;
  try {
    newTabId = await newTab({ host, hostGroupId: groupId, after: tab.id, cwd: landing.cwd, show: wasInView });
  } catch (e) {
    await explain(`Could not move "${title}" to ${name}.`, `The Host did not open a Tab: ${reasonOf(e)}. Nothing changed here.`);
    return;
  }
  closeTab(tab.id);
  if (!wasInView) markUnread(host, newTabId);

  // The conversation, once the dying Claude Code has stopped writing it.
  let conversationMoved = false;
  let reason = "";
  if (probe.entry?.kind === "claude" && landing.kind !== "home") {
    const target = landing.kind === "checkout" ? landing.cwd : landing.path;
    if (probe.conversation) {
      try {
        const files = await handoffConversationRead(probe.conversation);
        await hostPutConversation(host, target, probe.conversation.id, files);
        conversationMoved = true;
        await handoffConversationForget(probe.conversation).catch((e) => console.warn("handoff: the Mac copy stays", e));
      } catch (e) {
        reason = reasonOf(e);
      }
    } else reason = "its transcript was not found on this Mac";
  } else if (probe.entry?.kind === "claude") reason = `${name} has no checkout for it`;

  const newSession = await sessionOfTab(newTabId);
  if (newSession === null) {
    await explain(`"${title}" moved to ${name}, but its Tab has no Session.`, "Nothing was typed there.");
    return;
  }
  const note = handoffNote({ repo: git?.repoName ?? null, branch, id: probe.conversation?.id ?? "", reason });
  const input = handoffInput({ landing, entry: probe.entry, localRoot: git?.worktreeRoot ?? null, branch, cloneUrl: probe.git?.remoteUrl ?? null, conversationMoved, note });
  try {
    await typeInto(host, newSession, input);
  } catch (e) {
    await explain(`"${title}" moved to ${name}, but nothing could be typed there.`, e instanceof Error ? e.message : String(e));
    return;
  }
  if (probe.entry?.kind === "claude" && !conversationMoved) {
    await explain(
      `The conversation stayed on this Mac.`,
      `${reason ? reason[0].toUpperCase() + reason.slice(1) + ". " : ""}A fresh claude with a handoff note was typed on ${name} for you to finish; the transcript is still under this Mac's ~/.claude/projects.`,
    );
  }
}

/** The Session id of a Tab once the layout names it with its Host's Session; null after a moment without one. */
function sessionOfTab(tabId: string): Promise<number | null> {
  return new Promise((resolve) => {
    const start = Date.now();
    const tick = () => {
      const t = layout.tabs[tabId];
      if (t?.sessionId != null) resolve(t.sessionId);
      else if (Date.now() - start > SHELL_WAIT_MS) resolve(null);
      else setTimeout(tick, 50);
    };
    tick();
  });
}

/**
 * "Type git push first": `git push -u origin HEAD` typed, not run, into the Tab's own shell
 * when it is at its prompt, else into a new local Tab at the same directory; the Move is off
 * until the user asks again.
 */
async function typePushFirst(tab: Tab, probe: HandoffProbe): Promise<void> {
  if (tab.sessionId !== null && probe.info.shellIsForeground) {
    await writeSession(tab.sessionId, pushTyped()).catch(() => {});
    activateTab(tab.id);
    return;
  }
  const cwd = probe.info.cwd ?? undefined;
  const id = await newTab({ host: LOCAL_HOST, groupId: tab.groupId, cwd });
  const made = layout.tabs[id];
  if (made?.sessionId != null) {
    const sid = made.sessionId;
    setTimeout(() => void writeSession(sid, pushTyped()).catch(() => {}), 300);
  }
}
