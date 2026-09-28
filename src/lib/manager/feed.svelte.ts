// Every Host's agent events (what its agents did: tools used, questions asked, answers given,
// a screen-only agent's changes of status): an agent's name takes its few words from the last
// prompt among them, and Manager's finished rows what the last turn changed. The local Host's come from
// `agent_events` and the `agent-event` event; a paired Host's from its `hello` and
// `agent_event` messages (src/lib/host/hosts.svelte.ts). Each Host keeps its last 500
// (src-tauri/core/src/agents/); so does this client, per Host.

import { agentEvents, onAgentEvent } from "../ipc";
import { LOCAL_HOST, type HostId } from "../host/ids";
import type { AgentEvent } from "../types";

/** Events kept per Host, as the Host keeps them. */
const PER_HOST = 500;

/** One event, with the Host its Session is on. */
export interface HostAgentEvent extends AgentEvent {
  host: HostId;
}

export const feed = $state<{ byHost: Record<HostId, HostAgentEvent[]> }>({ byHost: {} });

/** A Host's events as it sent them in `hello` (oldest first): they replace what this client had. */
export function setHostAgentEvents(host: HostId, events: AgentEvent[]): void {
  feed.byHost[host] = events.slice(-PER_HOST).map((e) => ({ ...e, host }));
}

export function addAgentEvent(host: HostId, event: AgentEvent): void {
  const list = (feed.byHost[host] ??= []);
  list.push({ ...event, host });
  if (list.length > PER_HOST) list.splice(0, list.length - PER_HOST);
}

export function dropHostAgentEvents(host: HostId): void {
  delete feed.byHost[host];
}

/** Read the local Host's events and follow them; returns the function that stops. */
export function initAgentFeed(): () => void {
  let stopped = false;
  // Listen first: an event between the two reads is kept (and a duplicate is dropped below).
  const listening = onAgentEvent((e) => addAgentEvent(LOCAL_HOST, e));
  void agentEvents()
    .then((events) => {
      if (stopped) return;
      const seen = feed.byHost[LOCAL_HOST] ?? [];
      const merged = [...events];
      for (const e of seen) {
        if (!events.some((x) => x.at === e.at && x.sessionId === e.sessionId && x.text === e.text)) merged.push(e);
      }
      setHostAgentEvents(LOCAL_HOST, merged.sort((a, b) => a.at - b.at));
    })
    .catch((e) => console.error("manager: reading agent events failed", e));
  return () => {
    stopped = true;
    void listening.then((stop) => stop());
  };
}
