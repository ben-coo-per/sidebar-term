// Why a paired Host is slow, in one line for its chip on linked Tabs and Settings > Hosts: its
// Sessions short of memory (the Host's `pressure` samples, naming the Session using the most),
// or a connection that has stopped answering for a while without dropping (`slow`).

import type { HostPressure, SessionId } from "../types";
import { formatBytes } from "../panel/activity/model";

export interface Strain {
  /** `slow`: late answers with no pressure to explain them. */
  level: "slow" | "tight" | "throttled";
  text: string;
}

/**
 * What to say about a Host, or null while it is fine. `titleOf` names a Session of that Host
 * (its Tab's Title), or null when no Tab here shows it.
 */
export function describeStrain(
  pressure: HostPressure | null,
  slow: boolean,
  titleOf: (sessionId: SessionId) => string | null,
): Strain | null {
  const slowNote = slow ? " It is slow to answer." : "";
  if (!pressure || pressure.level === "ok") {
    return slow ? { level: "slow", text: "Slow to answer: busy, or the connection is poor." } : null;
  }
  const limit = pressure.high ?? pressure.max;
  const used = limit !== null ? `${formatBytes(pressure.mem)} of ${formatBytes(limit)}` : formatBytes(pressure.mem);
  const swap =
    pressure.swapMax !== null && pressure.swapMax > 0 && pressure.swap * 10 >= pressure.swapMax * 9
      ? ", swap full"
      : pressure.swap > 0
        ? `, ${formatBytes(pressure.swap)} in swap`
        : "";
  const top = pressure.top[0];
  const most = top ? ` Using the most: ${titleOf(top.sessionId) ?? `Session ${top.sessionId}`}, ${formatBytes(top.mem)}.` : "";
  const head = pressure.level === "throttled" ? "Out of memory for its Sessions" : "Memory is tight";
  return {
    level: pressure.level,
    text: `${head}: they use ${used}${swap}, waiting on memory ${Math.round(pressure.full)}% of the time.${most}${slowNote}`,
  };
}
