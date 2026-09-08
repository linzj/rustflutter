#!/usr/bin/env python3
"""Reclaim the build cache the workspace's own churn orphans.

    python3 tools/dart2rust/bin/prune_target.py [--ws .crate-ws] [--dry-run]

Incremental compilation earns its keep here -- chains 719-723 spent 151-224s
of `cargo` per run with it and 725 spent 347s without -- but nothing reclaims
it, and by run 721 `target/debug` held 248 GB of which under 16 GB was live.
Neither cargo nor rustc is at fault: every round re-partitions the workspace,
so crate names come and go and the surviving ones get a fresh `-C metadata`,
and a build tool only reclaims the units it still recognises. What it no
longer recognises it simply leaves.

Four kinds of garbage, measured on 2026-09-08:

  * incremental directories for crate names the current workspace no longer
    has -- 99 of the 163 names in it, 82 GB. Older partitionings named crates
    `gallery_above`, `scc_gallery_0`, `merged_cupertino_material_scc`;
    nothing will ever ask for those again.
  * `s-*-working` session directories, 255 of them, 7 GB: a session rustc was
    still writing when the OOM guard's `pkill -9` arrived. rustc finalises a
    session by renaming it; an unrenamed one is never read again.
  * older `-<disambiguator>` directories of crates that *do* still exist --
    `merged_gallery_scc` had 32, at 2.4 GB each (`dep-graph.bin` 1,335 MB,
    `query-cache.bin` 650 MB, `metadata.rmeta` 120 MB). One is live.
  * stale `deps/` artifacts and `.fingerprint/` units for hashes no longer
    referenced, 20 GB, plus the `.long-type-*.txt` diagnostic dumps, which
    are nothing in bytes and 67,776 files in inodes.

Deleting a live artifact by mistake is not a correctness problem -- cargo
rebuilds it -- so this errs toward deleting, except that it refuses to run
while a compile is in flight.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)

# How many `-<hash>` artifact variants to keep per crate name. A crate has a
# `cargo check` unit and a `cargo build` unit, which carry different
# extra-filenames; three leaves a spare.
KEEP_VARIANTS = 3
# Nothing younger than this is touched, so a compile that starts underneath
# this one still finds what it opened.
MIN_AGE_S = 15 * 60

ARTIFACT = re.compile(r'^(?:lib)?(.+)-([0-9a-f]{16})(\..*)?$')


# How long to wait for a compile to finish before giving up. The chain calls
# this at its start, which is between its own compiles but not necessarily
# between the outer loop's: the first call skipped outright because a `cargo
# build` from run_main.sh was still going. Waiting a little is what makes the
# reclamation actually happen.
BUSY_WAIT_S = 180


def busy():
    for name in ('rustc', 'cargo'):
        if subprocess.run(['pgrep', '-x', name],
                          stdout=subprocess.DEVNULL).returncode == 0:
            return name
    return None


def wait_quiet():
    deadline = time.time() + BUSY_WAIT_S
    who = busy()
    while who and time.time() < deadline:
        time.sleep(2)
        who = busy()
    return who


def du(path):
    total = 0
    for root, _, files in os.walk(path):
        for f in files:
            try:
                total += os.lstat(os.path.join(root, f)).st_size
            except OSError:
                pass
    return total


def gb(n):
    return n / float(1 << 30)


class Reaper(object):
    def __init__(self, dry):
        self.dry = dry
        self.freed = 0
        self.files = 0

    def rm(self, path, size=None):
        if size is None:
            size = du(path) if os.path.isdir(path) else os.path.getsize(path)
        self.freed += size
        self.files += 1
        if self.dry:
            return
        if os.path.isdir(path) and not os.path.islink(path):
            shutil.rmtree(path, ignore_errors=True)
        else:
            try:
                os.remove(path)
            except OSError:
                pass


def prune_incremental(ws, reaper, now):
    inc = os.path.join(ws, 'target/debug/incremental')
    if not os.path.isdir(inc):
        return
    members = {d for d in os.listdir(ws)
               if os.path.isfile(os.path.join(ws, d, 'Cargo.toml'))}
    by_name = {}
    for d in sorted(os.listdir(inc)):
        p = os.path.join(inc, d)
        if not os.path.isdir(p):
            continue
        name = d.rsplit('-', 1)[0]
        try:
            mtime = os.path.getmtime(p)
        except OSError:
            continue
        by_name.setdefault(name, []).append((mtime, p))

    dead = 0
    for name, entries in by_name.items():
        entries.sort(reverse=True)
        # A name the workspace no longer has: all of it goes.
        keep = 0 if name not in members else 1
        for mtime, p in entries[keep:]:
            if now - mtime < MIN_AGE_S:
                continue
            reaper.rm(p)
            dead += 1
        for _, p in entries[:keep]:
            prune_sessions(p, reaper, now)
    print('  incremental: dropped %d directories' % dead)


def prune_sessions(crate_dir, reaper, now):
    """Inside a kept crate directory: one finalised session, no `-working`."""
    finalised = []
    for s in os.listdir(crate_dir):
        p = os.path.join(crate_dir, s)
        if s.endswith('.lock'):
            continue
        if not os.path.isdir(p):
            continue
        try:
            mtime = os.path.getmtime(p)
        except OSError:
            continue
        if now - mtime < MIN_AGE_S:
            return          # something is using this crate; leave it alone
        if s.endswith('-working'):
            reaper.rm(p)    # rustc never renamed it: killed mid-session
        else:
            finalised.append((mtime, p))
    finalised.sort(reverse=True)
    for _, p in finalised[1:]:
        reaper.rm(p)


def prune_deps(ws, reaper, now):
    deps = os.path.join(ws, 'target/debug/deps')
    if not os.path.isdir(deps):
        return
    units, newest = {}, {}
    for f in os.listdir(deps):
        p = os.path.join(deps, f)
        if f.endswith('.txt') and '.long-type-' in f:
            if now - os.path.getmtime(p) > MIN_AGE_S:
                reaper.rm(p)
            continue
        m = ARTIFACT.match(f)
        if not m:
            continue
        key = (m.group(1), m.group(2))
        try:
            st = os.lstat(p)
        except OSError:
            continue
        units.setdefault(key, []).append(p)
        newest[key] = max(newest.get(key, 0), st.st_mtime)

    by_name = {}
    for name, h in units:
        by_name.setdefault(name, []).append(h)
    live = {}
    dropped = 0
    for name, hashes in by_name.items():
        hashes.sort(key=lambda h: newest[(name, h)], reverse=True)
        live[name] = max(newest[(name, h)] for h in hashes[:KEEP_VARIANTS])
        for h in hashes[KEEP_VARIANTS:]:
            if now - newest[(name, h)] < MIN_AGE_S:
                continue
            for p in units[(name, h)]:
                reaper.rm(p)
            dropped += 1
    print('  deps: dropped %d artifact variants' % dropped)

    fp = os.path.join(ws, 'target/debug/.fingerprint')
    if not os.path.isdir(fp):
        return
    stale = 0
    for d in os.listdir(fp):
        p = os.path.join(fp, d)
        m = re.match(r'^(.+)-[0-9a-f]{8,}$', d)
        if not m or not os.path.isdir(p):
            continue
        try:
            mtime = os.path.getmtime(p)
        except OSError:
            continue
        if now - mtime < MIN_AGE_S:
            continue
        # Its own hash is cargo's metadata hash, not the extra-filename above,
        # so age against the newest artifact of the same crate name is the
        # only join available.
        if mtime < live.get(m.group(1), 0) - 60:
            reaper.rm(p)
            stale += 1
    print('  .fingerprint: dropped %d unit directories' % stale)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--ws', default=os.path.join(TOOL, '.crate-ws'))
    ap.add_argument('--dry-run', action='store_true')
    args = ap.parse_args()

    who = wait_quiet()
    if who:
        print('prune_target: a %s is still running after %ds; nothing touched'
              % (who, BUSY_WAIT_S))
        return 0

    target = os.path.join(args.ws, 'target')
    before = du(target) if os.path.isdir(target) else 0
    reaper = Reaper(args.dry_run)
    now = time.time()
    prune_incremental(args.ws, reaper, now)
    prune_deps(args.ws, reaper, now)
    print('prune_target: %s %.1f GB in %d entries (target was %.1f GB)'
          % ('would free' if args.dry_run else 'freed',
             gb(reaper.freed), reaper.files, gb(before)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
