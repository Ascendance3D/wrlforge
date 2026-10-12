#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""PKG-LINUX-1: write the license texts of every third-party Rust crate that
the packaged application is built from.

Usage: collect-rust-licenses.py WORKSPACE_MANIFEST OUT_FILE

Reads `cargo metadata --locked --offline` twice: once filtered for the native
target (the `wrl-forge` binary) and once for wasm32 (the Leptos UI). The union
of both resolve graphs, minus workspace/path packages (WRL Forge's own
GPL-3.0-or-later code), is written with each crate's SPDX expression and the
LICENSE / COPYING / NOTICE files from its published source. Build-only crates
are included too: over-inclusion is safe, omission is not.

A crate without any license file is listed with its SPDX expression, its
authors and a note; the script never invents license text.
"""
import json
import os
import subprocess
import sys

TARGETS = ("x86_64-unknown-linux-gnu", "wasm32-unknown-unknown")
ROOTS = {"wrl-forge-desktop", "wrlforge-ui"}
PREFIXES = ("license", "licence", "copying", "notice", "copyright", "unlicense")


def metadata(manifest, target):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
         "--manifest-path", manifest, "--filter-platform", target],
        check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def reachable(meta):
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    by_id = {p["id"]: p for p in meta["packages"]}
    todo = [i for i, p in by_id.items() if p["name"] in ROOTS]
    seen = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        todo.extend(d["pkg"] for d in nodes[i]["deps"])
    return [by_id[i] for i in seen]


def license_files(pkg):
    root = os.path.dirname(pkg["manifest_path"])
    found = []
    for name in sorted(os.listdir(root)):
        path = os.path.join(root, name)
        if os.path.isfile(path) and name.lower().startswith(PREFIXES):
            found.append((name, path))
    lf = pkg.get("license_file")
    if lf:
        path = os.path.normpath(os.path.join(root, lf))
        if os.path.isfile(path) and all(p != path for _, p in found):
            found.append((lf, path))
    return found


def main():
    manifest, out_file = sys.argv[1], sys.argv[2]
    pkgs = {}
    for t in TARGETS:
        for p in reachable(metadata(manifest, t)):
            if p["source"] is None:  # workspace / path crate: WRL Forge's own code
                continue
            pkgs[(p["name"], p["version"])] = p
    with open(out_file, "w", encoding="utf-8") as f:
        f.write("WRL Forge (Tauri/Rust desktop) - third-party Rust crate licenses\n")
        f.write("Generated at build time from Cargo.lock by "
                "apps/desktop-tauri/packaging/collect-rust-licenses.py.\n")
        f.write(f"Crates: {len(pkgs)}\n\n")
        for key in sorted(pkgs):
            p = pkgs[key]
            f.write("=" * 78 + "\n")
            f.write(f"{p['name']} {p['version']}\n")
            f.write(f"License: {p.get('license') or '(not declared; see files)'}\n")
            if p.get("authors"):
                f.write(f"Authors: {', '.join(p['authors'])}\n")
            if p.get("repository"):
                f.write(f"Source: {p['repository']}\n")
            files = license_files(p)
            if not files:
                f.write("(no license file in the published crate; the standard "
                        "text of the license above applies, with the authors "
                        "listed as copyright holders)\n\n")
                continue
            for name, path in files:
                f.write("-" * 78 + f"\n{name}\n" + "-" * 78 + "\n")
                with open(path, encoding="utf-8", errors="replace") as lf:
                    f.write(lf.read().rstrip() + "\n")
            f.write("\n")
    print(f"{len(pkgs)} crates -> {out_file}")


if __name__ == "__main__":
    main()
