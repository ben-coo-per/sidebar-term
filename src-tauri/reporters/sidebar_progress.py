"""sidebar-term's pytest plugin: reports each test to the Suite progress file so the Tab shows
how far the run is. Loaded by the environment sidebar-term gives every Session
(PYTEST_ADDOPTS='-p sidebar_progress', PYTHONPATH=<this directory>); does nothing without
SIDEBAR_TERM_PROGRESS_DIR, and stays quiet in pytest-xdist workers (the controller reports every
test). Never touches pytest's own output. Protocol: docs/architecture.md "Suites"."""

import json
import os
import time


class _Writer:
    def __init__(self, path):
        self._file = open(path, "a", buffering=1)

    def emit(self, **fields):
        fields["t"] = int(time.time() * 1000)
        try:
            self._file.write(json.dumps(fields) + "\n")
            self._file.flush()
        except OSError:
            pass


class _Reporter:
    def __init__(self, writer):
        self._writer = writer

    def pytest_collection_finish(self, session):
        self._writer.emit(ev="total", total=len(session.items))

    def pytest_runtest_logreport(self, report):
        # One line per test: its call, or a setup that failed or skipped it (xfail reads as
        # skipped; an error is a failure).
        if report.when == "call" or (report.when == "setup" and report.outcome != "passed"):
            outcome = report.outcome if report.outcome in ("passed", "failed", "skipped") else "failed"
            self._writer.emit(ev="case", status=outcome)

    def pytest_sessionfinish(self, session, exitstatus):
        self._writer.emit(ev="end", status="passed" if exitstatus == 0 else "failed")


def pytest_configure(config):
    directory = os.environ.get("SIDEBAR_TERM_PROGRESS_DIR")
    if not directory or os.environ.get("PYTEST_XDIST_WORKER"):
        return
    try:
        writer = _Writer(os.path.join(directory, "%d.jsonl" % os.getpid()))
    except OSError:
        return
    writer.emit(ev="start", runner="pytest", pid=os.getpid())
    config.pluginmanager.register(_Reporter(writer), "sidebar_progress_reporter")
