// The Resume banner's state: what the last run left running in each Tab (Rust's `resume.json`,
// read with `resume_leftover`), and the actions that start it again by typing into the Tab's
// shell. See docs/architecture.md "Resume" and src/lib/resume/model.ts for the pure rules.

import { untrack } from "svelte";
import { resumeForget, resumeLeftover, sessionInfo, writeSession } from "../ipc";
import { layout } from "../layout.svelte";
import { sessionState } from "../sessions.svelte";
import type { ResumeEntry } from "../types";
import { resumeInput } from "./model";

interface ResumeState {
  /** The banner's rows, in the order Rust recorded them. The banner shows while there are any. */
  entries: ResumeEntry[];
  /** Keys of entries last skipped because their Tab was not at its prompt. */
  busy: string[];
}

export const resume = $state<ResumeState>({ entries: [], busy: [] });

/**
 * Load what the last run left running. Call once the layout has loaded: an entry whose Tab no
 * longer exists is forgotten.
 */
export async function initResume(): Promise<void> {
  const leftover = await resumeLeftover().catch(() => [] as ResumeEntry[]);
  forget(leftover.filter((e) => !layout.tabs[e.key]).map((e) => e.key));
  resume.entries = leftover.filter((e) => layout.tabs[e.key]);
}

/**
 * Start `entries` again, each by typing its line into its Tab's shell. A Tab whose shell is not
 * at its prompt (running something already) is never typed into: its entry stays, marked busy.
 */
export async function resumeEntries(entries: ResumeEntry[]): Promise<void> {
  const done: string[] = [];
  const busy: string[] = [];
  for (const e of entries) {
    const sessionId = layout.tabs[e.key]?.sessionId ?? null;
    const info = sessionId === null ? null : await sessionInfo(sessionId).catch(() => null);
    if (sessionId === null || !info?.shellIsForeground) {
      busy.push(e.key);
      continue;
    }
    await writeSession(sessionId, resumeInput(e, info.cwd));
    done.push(e.key);
  }
  resume.busy = busy;
  forget(done);
}

/** Close the banner without resuming anything. */
export function dismissResume(): void {
  forget(resume.entries.map((e) => e.key));
}

function forget(keys: string[]): void {
  if (keys.length === 0) return;
  resume.entries = resume.entries.filter((e) => !keys.includes(e.key));
  resume.busy = resume.busy.filter((k) => !keys.includes(k));
  void resumeForget(keys).catch(() => {});
}

// An entry is moot once its Tab is closed, or runs an agent (resumed by hand). Anything else
// the Tab runs does not count: shell startup files run commands too.
$effect.root(() => {
  $effect(() => {
    const moot = resume.entries.filter((e) => {
      const tab = layout.tabs[e.key];
      return !tab || Boolean(sessionState(tab.sessionId)?.info?.agent);
    });
    if (moot.length > 0) untrack(() => forget(moot.map((e) => e.key)));
  });
});
