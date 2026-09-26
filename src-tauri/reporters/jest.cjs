// sidebar-term's jest reporter: one line per test to the Suite progress file so the Tab shows
// how far the run is. jest reads no reporter from the environment, so add it to the project's
// config, next to the reporters it already has:
//
//   // jest.config.js
//   module.exports = {
//     reporters: ["default", ...(process.env.SIDEBAR_TERM_REPORTERS ? [`${process.env.SIDEBAR_TERM_REPORTERS}/jest.cjs`] : [])],
//   };
//
// jest does not know the number of tests up front, so the Tab counts tests done and takes its
// ETA from earlier runs. Does nothing without SIDEBAR_TERM_PROGRESS_DIR and never writes to the
// terminal. Protocol: docs/architecture.md "Suites".
"use strict";

const fs = require("node:fs");

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
  switch (result.status) {
    case "passed":
      return "passed";
    case "failed":
      return "failed";
    default:
      return "skipped"; // skipped, pending, todo, disabled
  }
}

class SidebarProgress {
  constructor() {
    this.log = openLog();
    /** Cases already reported per test file, so a file's summary only adds what is missing. */
    this.reported = new Map();
  }

  onRunStart() {
    this.reported.clear();
    this.log({ ev: "start", runner: "jest", pid: process.pid });
  }

  onTestCaseResult(test, result) {
    this.reported.set(test.path, (this.reported.get(test.path) ?? 0) + 1);
    this.log({ ev: "case", status: status(result) });
  }

  onTestFileResult(test, result) {
    // Skipped tests get no onTestCaseResult; the file's summary lists them all in order.
    const seen = this.reported.get(test.path) ?? 0;
    for (const r of result.testResults.slice(seen)) this.log({ ev: "case", status: status(r) });
  }

  onRunComplete(contexts, results) {
    this.log({
      ev: "end",
      status: results.success ? "passed" : "failed",
      passed: results.numPassedTests,
      failed: results.numFailedTests,
    });
  }

  getLastError() {}
}

module.exports = SidebarProgress;
