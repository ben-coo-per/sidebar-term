// sidebar-term's mocha reporter: the reporter the project configured (`.mocharc`'s `reporter`,
// else Spec), plus one line per test to the Suite progress file so the Tab shows how far the run
// is. Loaded by the environment sidebar-term gives every Session
// (MOCHA_OPTIONS='--reporter <this file>'); mocha takes one reporter, hence the wrapping. Does
// nothing extra without SIDEBAR_TERM_PROGRESS_DIR. Protocol: docs/architecture.md "Suites".
"use strict";

const fs = require("node:fs");
const path = require("node:path");

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

// The project's mocha, not one next to this file: resolve from the working directory (the bin
// mocha ran from is `require.main`).
function projectMocha() {
  const paths = [process.cwd()];
  if (require.main?.filename) paths.push(path.dirname(require.main.filename));
  return require(require.resolve("mocha", { paths }));
}

// The reporter the project configured in its .mocharc, if any; ours stands in for it.
function configuredReporter(Mocha) {
  try {
    const { loadRc } = require(require.resolve("mocha/lib/cli/options", { paths: [process.cwd()] }));
    const name = loadRc()?.reporter;
    if (!name || name === "spec") return null;
    if (Mocha.reporters[name]) return Mocha.reporters[name];
    return require(require.resolve(name, { paths: [process.cwd()] }));
  } catch {
    return null;
  }
}

const Mocha = projectMocha();
const Base = configuredReporter(Mocha) ?? Mocha.reporters.Spec;

class SidebarProgress extends Base {
  constructor(runner, options) {
    super(runner, options);
    const log = openLog();
    runner.on("start", () => {
      log({ ev: "start", runner: "mocha", pid: process.pid });
      log({ ev: "total", total: runner.total });
    });
    runner.on("pass", () => log({ ev: "case", status: "passed" }));
    runner.on("fail", () => log({ ev: "case", status: "failed" }));
    runner.on("pending", () => log({ ev: "case", status: "skipped" }));
    runner.on("end", () => {
      const stats = runner.stats ?? {};
      log({
        ev: "end",
        status: (stats.failures ?? 0) > 0 ? "failed" : "passed",
        passed: stats.passes ?? 0,
        failed: stats.failures ?? 0,
      });
    });
  }
}

module.exports = SidebarProgress;
