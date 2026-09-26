// sidebar-term's vitest reporter (reporter API v3, vitest 3+): one line per test to the Suite
// progress file so the Tab shows how far the run is. vitest reads no reporter from the
// environment, so add it to the project's config, next to the reporters it already has:
//
//   // vitest.config.ts
//   export default defineConfig({
//     test: {
//       reporters: ["default", ...(process.env.SIDEBAR_TERM_REPORTERS ? [`${process.env.SIDEBAR_TERM_REPORTERS}/vitest.mjs`] : [])],
//     },
//   });
//
// Does nothing without SIDEBAR_TERM_PROGRESS_DIR and never writes to the terminal.
// Protocol: docs/architecture.md "Suites".

import fs from "node:fs";

function openLog() {
  const dir = process.env.SIDEBAR_TERM_PROGRESS_DIR;
  if (!dir) return () => {};
  try {
    const fd = fs.openSync(`${dir}/${process.pid}.jsonl`, "a");
    return (fields) => {
      try {
        fs.writeSync(fd, JSON.stringify({ t: Date.now(), ...fields }) + "\n");
      } catch {
        /* the Session is gone */
      }
    };
  } catch {
    return () => {};
  }
}

function status(result) {
  switch (result?.state) {
    case "passed":
      return "passed";
    case "failed":
      return "failed";
    default:
      return "skipped";
  }
}

export default class SidebarProgress {
  log = openLog();
  reported = new Set();

  onTestRunStart() {
    this.reported.clear();
    this.log({ ev: "start", runner: "vitest", pid: process.pid });
  }

  onTestModuleCollected(module) {
    // `allTests()` is an iterable, not an array.
    this.log({ ev: "total", total: Array.from(module.children.allTests()).length });
  }

  onTestCaseResult(testCase) {
    this.reported.add(testCase.id);
    this.log({ ev: "case", status: status(testCase.result()) });
  }

  onTestRunEnd(modules, errors, reason) {
    let passed = 0;
    let failed = 0;
    for (const module of modules) {
      for (const test of module.children.allTests()) {
        const s = status(test.result());
        // Tests vitest never reported (skipped ones) still count against the total.
        if (!this.reported.has(test.id)) this.log({ ev: "case", status: s });
        if (s === "passed") passed++;
        if (s === "failed") failed++;
      }
    }
    const ok = reason === "passed" && failed === 0 && errors.length === 0;
    this.log({ ev: "end", status: ok ? "passed" : "failed", passed, failed });
  }
}
