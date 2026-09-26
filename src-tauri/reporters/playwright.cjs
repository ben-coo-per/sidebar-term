// sidebar-term's Playwright Test reporter: reports each test to the Suite progress file so the
// Tab shows how far the run is. Loaded by the environment sidebar-term gives every Session
// (PW_TEST_REPORTER=<this file>), in addition to the reporters the project configures; does
// nothing without SIDEBAR_TERM_PROGRESS_DIR and never writes to the terminal.
// Protocol: docs/architecture.md "Suites".
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

class SidebarProgress {
  constructor() {
    this.log = openLog();
  }

  printsToStdio() {
    return false;
  }

  onBegin(config, suite) {
    this.log({ ev: "start", runner: "playwright", pid: process.pid });
    this.log({ ev: "total", total: suite.allTests().length });
  }

  onTestEnd(test, result) {
    // A failure that will be retried is not the test's result yet.
    const final = result.status === "passed" || result.status === "skipped" || result.retry >= test.retries;
    if (!final) return;
    this.log({ ev: "case", status: result.status });
  }

  onEnd(result) {
    this.log({ ev: "end", status: result.status === "passed" ? "passed" : "failed" });
  }
}

module.exports = SidebarProgress;
