# Contributing to WRL Forge

Thank you for your interest in WRL Forge. Community input is genuinely valued —
especially bug reports, VRML compatibility findings, code, and documentation fixes.

WRL Forge is **free and open-source software** under the **GNU General Public License,
version 3 or later**. Contributions are welcome.

## Licensing and copyright

- Contributions to WRL Forge's own software and documentation are made under
  **`GPL-3.0-or-later`** — the same licence as the project ([`LICENSE`](LICENSE)).
- **You keep the copyright in what you write.** WRL Forge does **not** require copyright
  assignment, and there is **no Contributor Licence Agreement (CLA)** to sign.
- Third-party material you bring in keeps *its* copyright and licence, and must be
  compatible with distributing WRL Forge under `GPL-3.0-or-later`. See
  [`OPEN_SOURCE_PROVENANCE.md`](OPEN_SOURCE_PROVENANCE.md).
- Artwork and VRML content are a separate matter — see
  [`FEATURED_CONTENT_POLICY.md`](FEATURED_CONTENT_POLICY.md). Artists are **not** asked
  to GPL their art.

## Sign your work — the DCO

WRL Forge uses the **Developer Certificate of Origin** (DCO) rather than a CLA. Sign off
each commit:

```
git commit -s
```

which appends a line to the commit message:

```
Signed-off-by: Your Name <your.email@example.com>
```

That line certifies you have the right to submit the work under the project's licence —
that you wrote it, or that it came from a source whose licence permits you to
contribute it, and that you understand the contribution is public and recorded.

The full canonical text is short and worth reading once:
**<https://developercertificate.org/>**

Sign-off applies to contributions made **from the GPL transition forward**. Commits made
before it are simply from a period when the project had no sign-off policy — there is
nothing wrong with them and nothing to retrofit.

## Contributing third-party code

If your contribution copies, adapts, ports, or closely translates code from another
open-source project, say so **in the pull request**, and include:

- the upstream project and its source URL;
- the version, tag, or commit you took it from;
- which files or components you used;
- the upstream licence (SPDX identifier);
- confirmation that upstream copyright and licence notices are preserved in the files.

Preserve upstream headers verbatim. Never replace someone else's copyright line with a
WRL Forge one. Material with unknown or unverifiable origin cannot be accepted — see
[`OPEN_SOURCE_PROVENANCE.md`](OPEN_SOURCE_PROVENANCE.md) §3.

## Ways to contribute

- **Code** — bug fixes, features, platform support, performance work.
- **Bug reports** — something crashed, rendered wrong, or behaved unexpectedly.
- **VRML compatibility reports** — a `.wrl` file or world that WRL Forge parses,
  previews, or packages differently than the original platform did.
- **Documentation corrections** — typos, unclear steps, out-of-date instructions.
- **Reproduction files** — small sample `.wrl` files or worlds that demonstrate a
  problem… but only content you have the right to share (see the warning below).
- **Discussion of substantial changes** — for anything large, **raise it first** via
  Discussions or an issue, so scope and design can be talked through before you invest
  the time. This is courtesy, not a barrier.

Issue forms: <https://github.com/Ascendance3D/wrlforge/issues/new/choose>
Discussions: <https://github.com/Ascendance3D/wrlforge/discussions>

## Building and testing

WRL Forge requires **Node 20+**. There is no external test framework — tests run on
`node:test`.

```
npm install
npm run build:editor     # build the CodeMirror editor bundle
npm run check            # full project gate: tests + syntax checks
npm start                # run the app
```

Before opening a pull request:

- run the **focused tests** for the area you touched;
- run **`npm run check`** and make sure it passes in full;
- keep **fail-closed semantic gates** fail-closed. Parts of this codebase — node
  identity, scope resolution — are designed to answer *"I cannot prove that"* rather
  than guess. Do not relax one into a best-effort match to make a test pass;
- do not add a runtime dependency without discussing it first. The runtime dependency
  set is deliberately `x_ite` only.

See [`CLAUDE.md`](CLAUDE.md) and [`WD.md`](WD.md) for the architectural constraints that
a change is expected to respect.

## ⚠️ Do not upload proprietary or private content

Do **not** attach, commit, or otherwise share **proprietary or private Cybertown
content** — worlds, meshes, textures, scripts, or other assets — **unless you have
permission** to share it. When you need a sample to demonstrate a bug, use content you
created or are otherwise authorised to distribute. WRL Forge is an independent community
project and is **not affiliated with or endorsed by Cybertown**; respecting others'
content is a hard requirement.

## Security issues

Please do **not** report security vulnerabilities in public issues or Discussions.
Follow the private reporting process in [`SECURITY.md`](SECURITY.md).

## Getting help

If you just need help using WRL Forge rather than contributing, see
[`SUPPORT.md`](SUPPORT.md).
