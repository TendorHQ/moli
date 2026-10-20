#!/usr/bin/env python3
"""Run the original reproduction command with opt-in Moli and Linux diagnostics.

Example:
  python3 scripts/diagnose-ci-parallel.py -- cargo nextest run --workspace --profile ci -j8

The command, deadlines, retries and concurrency are not modified. No ptrace or
extra Python packages are required. Ctrl-C is forwarded to the command group.
"""

import argparse
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys
import tempfile
import threading
import time


def read(path):
    try:
        return Path(path).read_text(errors="replace").strip()
    except OSError:
        return None


def proc_stat(path):
    value = read(path)
    if not value:
        return None
    # comm may contain spaces and parentheses. Remaining fields start at #3.
    end = value.rfind(")")
    fields = value[end + 2:].split()
    try:
        return {
            "comm": value[value.index("(") + 1:end],
            "state": fields[0], "ppid": int(fields[1]),
            "minflt": int(fields[7]), "majflt": int(fields[9]),
            "utime_ticks": int(fields[11]), "stime_ticks": int(fields[12]),
            "priority": int(fields[15]), "nice": int(fields[16]),
            "threads": int(fields[17]), "starttime_ticks": int(fields[19]),
            "rss_pages": int(fields[21]), "processor": int(fields[36]),
        }
    except (IndexError, ValueError):
        return None


def proc_details(pid):
    root = Path("/proc") / str(pid)
    cmdline = read(root / "cmdline")
    return {
        "pid": pid, "argv": cmdline.split("\0") if cmdline else [],
        "status": read(root / "status"), "limits": read(root / "limits"),
        "cgroup": read(root / "cgroup"), "io": read(root / "io"),
    }


def cgroup_paths(pid):
    """Capture v2 limits at each ancestor; v1 controller paths are included too."""
    result = {Path("/sys/fs/cgroup")}
    for line in (read(f"/proc/{pid}/cgroup") or "").splitlines():
        _, controllers, relative = line.split(":", 2)
        roots = [Path("/sys/fs/cgroup")]
        if controllers:
            roots = [Path("/sys/fs/cgroup") / controllers]
            roots += [Path("/sys/fs/cgroup") / c for c in controllers.split(",")]
        for root in roots:
            path = root / relative.lstrip("/")
            # Ignore namespace-relative paths outside the mounted hierarchy.
            if ".." in path.parts:
                continue
            while path.is_relative_to(root):
                result.add(path)
                if path == root:
                    break
                path = path.parent
    return result


CGROUP_FILES = (
    "cpu.stat", "cpu.max", "cpu.weight", "cpu.pressure", "cpuset.cpus.effective",
    "cpu.cfs_quota_us", "cpu.cfs_period_us", "cpuacct.usage",
    "memory.current", "memory.max", "memory.high", "memory.events",
    "memory.swap.current", "memory.swap.max", "memory.pressure",
    "memory.usage_in_bytes", "memory.limit_in_bytes", "io.stat", "io.pressure",
    "pids.current", "pids.max", "pids.events",
)


def sample(root_pid, known, groups):
    processes = {}
    for path in Path("/proc").glob("[0-9]*/stat"):
        stat = proc_stat(path)
        if stat:
            processes[int(path.parent.name)] = stat
    descendants = {root_pid}
    while True:
        found = {pid for pid, stat in processes.items() if stat["ppid"] in descendants}
        if found <= descendants:
            break
        descendants.update(found)
    # Retain already observed descendants that have outlived/reparented from
    # their immediate parent; compare process start times to avoid PID reuse.
    descendants.update(pid for pid, start in known.items()
                       if pid in processes and processes[pid]["starttime_ticks"] == start)
    details, threads = [], []
    for pid in sorted(descendants & processes.keys()):
        if known.get(pid) != processes[pid]["starttime_ticks"]:
            known[pid] = processes[pid]["starttime_ticks"]
            details.append(proc_details(pid))
            groups.update(cgroup_paths(pid))
        for path in (Path("/proc") / str(pid) / "task").glob("[0-9]*"):
            stat = proc_stat(path / "stat")
            if stat:
                threads.append({
                    "pid": pid, "tid": int(path.name), **stat,
                    # schedstat: CPU runtime ns, runnable queue wait ns, slices.
                    "schedstat": read(path / "schedstat"),
                    "wchan": read(path / "wchan"),
                })
    for pid in sorted(descendants & processes.keys()):
        processes[pid]["io"] = read(f"/proc/{pid}/io")
        processes[pid]["status"] = read(f"/proc/{pid}/status")
    cgroups = {}
    for path in sorted(groups):
        values = {name: value for name in CGROUP_FILES
                  if (value := read(path / name)) is not None}
        if values:
            cgroups[str(path)] = values
    return {
        "new_processes": details, "threads": threads, "cgroups": cgroups,
        "processes": processes,  # Also records other Cargo/rustc work on host.
        "system": {name: read("/proc/" + name) for name in (
            "stat", "meminfo", "vmstat", "loadavg", "diskstats",
            "pressure/cpu", "pressure/memory", "pressure/io",
            "sys/fs/file-nr", "net/sockstat",
        )},
    }


def sampler(root_pid, output, interval, stop):
    known, groups = {}, set()
    previous = time.monotonic()
    with output.open("w", buffering=1) as stream:
        while not stop.is_set():
            started = time.monotonic()
            try:
                record = sample(root_pid, known, groups)
            except Exception as error:  # Keep the reproduction running if sampling fails.
                record = {"sampler_error": repr(error)}
            record.update(unix_us=time.time_ns() // 1000, monotonic=started,
                          interval_ms=(started - previous) * 1000,
                          sample_ms=(time.monotonic() - started) * 1000)
            stream.write(json.dumps(record, separators=(",", ":")) + "\n")
            previous = started
            stop.wait(max(0, interval - (time.monotonic() - started)))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, help="new capture directory (default: /tmp/moli-ci-diag-*)")
    parser.add_argument("--interval", type=float, default=0.25, help="Linux sampling period in seconds")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("supply the original reproduction command after --")
    if args.interval < 0.05:
        parser.error("--interval must be at least 0.05 seconds")
    if args.output:
        output = args.output.resolve()
        output.mkdir(parents=True, exist_ok=False)
    else:
        output = Path(tempfile.mkdtemp(prefix="moli-ci-diag-"))
    env = os.environ.copy()
    env["MOLI_DIAGNOSTICS_DIR"] = str(output)
    metadata = {
        "command": command, "cwd": os.getcwd(), "uname": list(os.uname()),
        "unix_us": time.time_ns() // 1000, "interval": args.interval,
        "clock_ticks": os.sysconf("SC_CLK_TCK"), "page_size": os.sysconf("SC_PAGE_SIZE"),
        "affinity": sorted(os.sched_getaffinity(0)),
        "kernel_schedstats": read("/proc/sys/kernel/sched_schedstats"),
        "boot_id": read("/proc/sys/kernel/random/boot_id"),
        "fd_limits": resource.getrlimit(resource.RLIMIT_NOFILE),
        "environment": {key: env.get(key) for key in (
            "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "CARGO_PROFILE_TEST_DEBUG",
            "CARGO_PROFILE_DEV_DEBUG", "CARGO_INCREMENTAL", "RUSTFLAGS",
            "NEXTEST_TEST_THREADS", "TOKIO_WORKER_THREADS", "RUST_MIN_STACK",
        )},
    }
    for key, git_args in (("commit", ["rev-parse", "HEAD"]), ("changes", ["status", "--short"])):
        result = subprocess.run(["git", *git_args], capture_output=True, text=True, check=False)
        metadata[key] = result.stdout.strip()
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Diagnostics: {output}", flush=True)
    child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, start_new_session=True)

    def forward(signum, _frame):
        try:
            os.killpg(child.pid, signum)
        except ProcessLookupError:
            pass

    signal.signal(signal.SIGINT, forward)
    signal.signal(signal.SIGTERM, forward)
    stop = threading.Event()
    worker = threading.Thread(target=sampler, args=(child.pid, output / "system.jsonl", args.interval, stop), daemon=True)
    worker.start()
    try:
        with (output / "nextest.log").open("wb") as stream:
            while chunk := child.stdout.read1(65536):
                stream.write(chunk)
                stream.flush()
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
        returncode = child.wait()
    finally:
        if child.poll() is None:
            forward(signal.SIGTERM, None)
            child.wait()
        stop.set()
        worker.join(timeout=5)
    metadata.update(returncode=returncode, finished_unix_us=time.time_ns() // 1000)
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"\nDiagnostics saved: {output}", flush=True)
    return returncode if returncode >= 0 else 128 - returncode


if __name__ == "__main__":
    sys.exit(main())
