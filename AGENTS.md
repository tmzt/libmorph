# libmorph

libmorph is a windowing, graphics and device-emulation shell for `wgpu`
applications: one `MorphApp` runs on desktop, the web and Android. What the
crate does and how an app uses it is documented in `README.md` and the
rustdoc, not here. This file carries the critical rules, and then what is
specific to this project.

# Critical rules for tmeade / tmzt projects

These rules apply to **every project created by tmeade (GitHub: tmzt)**:
the repositories under `~/src/` that are his own, and any new project
started for him. They do not apply to third-party repositories, forks of
other people's work, or upstream code vendored into a project. Their
canonical copy is `rules/CRITICAL_RULES.md` in `~/src/rules` on the Mac
mini (`timothys-mac-mini-1` on Tailscale), published at
`github.com/tmzt/rules`. A `~/src/rules` on any other machine is not the
canonical copy; every other copy, this one included if it is elsewhere, is
derived from it (see "Where these rules live").

## Where these rules live

1. The canonical copy of these rules is `rules/CRITICAL_RULES.md` in the
   public repository `github.com/tmzt/rules`, checked out at `~/src/rules`.
   Every other copy is derived from it: `~/.claude/rules/CRITICAL_RULES.md`
   and the start of each project's `AGENTS.md`.
2. Any change to these rules, by an owner or an agent, from any project, is
   made in `~/src/rules`, under that repository's own `AGENTS.md`, and then
   copied out. A derived copy is never edited in place; where one differs
   from the canonical copy, the canonical copy is right.
3. The rules are language-agnostic. What differs between languages (the
   commands for Rule 3.4, where Rule 5 keeps a version) is given here as
   examples, and each project names its own in its `AGENTS.md`.

## A project's `AGENTS.md`

A project's `AGENTS.md` starts with these rules, byte for byte, then a line
`---` and a section `# This project: <name>` that adds what is specific to
the project: its languages and layout, its packages, and its commands for
Rule 3.4 and Rule 5. The project section adds to the rules and is more
specific where it applies; it does not restate or change the rules text.

When a tmeade project has no `AGENTS.md` yet, create one that way, along
with an empty `CHANGES.md`, a `docs/design/` folder, a README laid out as
in Rule 4, and a `repowiki.toml` (see "Repowiki" below), each in its own
commit (Rule 3.1). For a library with a public API, also add `VERSIONS.md`
(Rule 5).

## CRITICAL RULE 0: THE CHANGES.MD PROTOCOL

Before writing, modifying or deleting code for a new planned work item:

1. Only planned new work, feature plans and architectural changes are
   appended to the bottom of `CHANGES.md`. Routine implementation work that
   follows an existing plan does not need its own addendum.
2. Each addendum is timestamped (`## YYYY-MM-DD HH:MM TZ — <subject>`, from the
   system clock) and names the plan it belongs to. Entries are appended, never
   edited.
3. Departures from a frozen plan (Rule 1.7) are recorded as addenda here.
4. Updates are small, incremental steps. No large, unannounced redesigns of
   the codebase.

## CRITICAL RULE 1: SPECS AND PLANS

1. Specs and plans live in `docs/design/`, named `YYYYMMDD_NN_NAME.md`. `NN` is
   the sequence within the day, and plans end in `_PLAN`. Each starts with YAML
   front matter: `title`, `date`, `status`, what it covers, its sources, and
   related or dependent documents.
2. Plans are written in plain engineering language that a person could build
   from alone: concrete interfaces, data layouts, numbers and paths, and
   numbered steps, each ending with **Done when:** stating what works at that
   point.
3. `status` is one of:
   - `Draft`: being worked out, changed only by amendments;
   - `Hold`: no work may start or continue until an amendment returns it to
     `Draft`;
   - `In implementation`: frozen.
   - `Implemented`: every step is done and its **Done when:** holds. It stays
     frozen, and living sections stay open. It is set by an amendment that lists
     the commits implementing each step, in the commit that completes the last
     step or in a later one.
   - `Superseded (by <plan>)`: replaced before implementation by the named
     plan, which carries its content forward. Nothing in it is implemented.
     It is set with an amendment naming the replacement and what moved where.
   - `Retired (by <plan>)`: implemented, in whole or in part, and then
     removed by the named plan; its code no longer exists. It is set with an
     amendment naming that plan, the commit that removed the code, and the
     last commit that still has it, so the code can be recovered from
     history (for example `git checkout <commit> -- <paths>`). It stays
     frozen.

   The front-matter `status` is the only part of a document edited in place,
   and every status change gets its own amendment.
4. An amendment is appended to the end of the document as
   `## Amendment — YYYY-MM-DD HH:MM TZ: <subject>`. The body is never rewritten.
   An amendment states which earlier text it replaces; where they disagree, the
   later amendment wins. Timestamps come from the system clock, never typed by
   hand, and a wrong one is corrected by a later note, not by editing.
5. The first commit that implements a plan sets `status: In implementation`
   and appends an amendment saying implementation has started and the document
   is now immutable. After that it changes only on the owner's explicit
   instruction.
6. A plan may name **living sections** in its front matter. These stay open
   after implementation starts: new entries are appended as amendments, and an
   entry is fixed once its item is implemented. Implementation records, such as
   which commits implemented an entry, are appended as amendments, not filled in
   place.
7. When implementation departs from a frozen plan:
   - small departures needed to make the plan work are recorded in an addendum
     to `CHANGES.md`, naming the plan, the step and the reason;
   - a departure that changes behaviour or an interface others depend on is
     discussed with the owner first (Rule 2), then recorded;
   - a frozen plan is never edited to match the code. A larger change of
     direction becomes a new dated plan that names the one it replaces.

## CRITICAL RULE 2: DISCUSS BEFORE CHANGING A PLAN

1. A change to a spec or plan is proposed to the owner first: what changes,
   where and why. Nothing is written to the document until the owner agrees.
2. Refinements made during the discussion go into the proposal, not the file.
   Once agreed, the whole change is written as **one** amendment, not a series
   of amendments that replace one another.

## CRITICAL RULE 3: COMMITS AND EVIDENCE

1. At most one package per commit: a Rust crate, a Go module, an npm
   package, a Python package, or another subproject the project's
   `AGENTS.md` names. Repository-level files go in their own commits:
   `AGENTS.md`, the workspace manifest (a Cargo workspace, `go.work`, a
   root `package.json` with workspaces), evidence logs, tooling config and
   scripts. The one exception is adding a package to the workspace in that
   package's first commit.
2. A plan's status change and its immutable marker go in the commit that starts
   implementing it.
3. Commits name their paths explicitly (`git add <paths>`,
   `git commit -- <paths>`), so unrelated work in progress, including another
   agent's, is never swept in. Commits already on the shared branch are never
   amended or rewritten.
4. Before each code commit, the changed package passes its formatter check,
   its linter with no new warnings, and its tests. The project's `AGENTS.md`
   names the exact commands. For example:

   | Language | Formatter check | Linter | Tests |
   | --- | --- | --- | --- |
   | Rust | `cargo fmt -p <crate> -- --check` | `cargo clippy -p <crate> --all-targets` | `cargo test -p <crate>` |
   | Go | `gofmt -l .` prints nothing | `go vet ./...` | `go test ./...` |
   | TypeScript | `prettier --check .` | `eslint .` and `tsc --noEmit` | `npm test` |
   | Python | `ruff format --check` | `ruff check` | `pytest` |
5. Renders and other generated evidence go under the gitignored `out/`. Each
   area keeps its own log file. Entries are appended under dated headings, with
   `./`-relative links and descriptions that stand on their own without the
   images. Earlier entries are never edited. Each run of evidence gets its
   own directory in its area, named like the plans: `YYYYMMDD_NN_name/`, with
   `NN` the sequence within the day (for example
   `out/fonts/20261023_01_cap_clipping/`).
6. Other documents under `docs/` that record findings, such as
   `docs/FONT_<NAME>.md`, are append-only. Each addition is a section headed
   `## YYYY-MM-DD HH:MM TZ — <subject>`, timestamped from the system clock.
   Earlier sections are never edited; a correction is a later dated section
   that says what it corrects.

## CRITICAL RULE 4: THE README AND ITS DECISION TABLE

1. `README.md` is laid out in this order, so every section has a fixed
   place:
   1. a short introduction: what the project is, in a paragraph or two;
   2. the rest of the README: usage, features, building, licences and so on;
   3. `## Architectural decisions`, last: one GitHub Markdown table listing
      every spec and plan in `docs/design/`, oldest first. Nothing follows
      the table.

   The critical rules live in `AGENTS.md` only; the README does not repeat
   or summarise them.
2. Columns: **Document** (the title, as a relative link such as
   `[Camera view and capture](docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md)`;
   no absolute or GitHub URLs), **Date** (from its front matter), **Status**
   (exactly its front-matter `status`, including `Superseded (by <link>)`
   and `Retired (by <link>)`),
   and **Decision** (one line: what it decides, and why it is at that status).
3. A row is added in the commit that adds the document, and updated in the
   same commit as every status-change amendment. This is an exception to
   Rule 3.1. The table is a summary: the documents and their amendments stay
   the record.

## CRITICAL RULE 5: VERSIONS AND TAGS

1. The commit that changes the project's version is tagged `v<version>`
   (for example `git tag v0.3.0 <commit>`) as soon as it is committed. A
   tag that has been pushed is never moved or deleted. Where the version is
   kept, for example: Rust, `version` in `Cargo.toml`; TypeScript,
   `version` in `package.json`; Python, `version` in `pyproject.toml`; Go,
   which keeps no version in `go.mod`, only the tag, so the version changes
   in the commit that adds its `VERSIONS.md` entry.
2. `VERSIONS.md` records each version change, and only version changes:
   one entry per version, appended oldest first, headed
   `## <version> — YYYY-MM-DD (<short commit>, tag v<version>)`. The entry
   lists the public API changes since the previous version: what was added,
   changed or removed, with breaking changes marked **Breaking**. Internal
   changes are not listed, and commits that do not change the version get no
   entry. Earlier entries are never edited.
3. The `VERSIONS.md` entry is in the same commit as the version change. This
   is an exception to Rule 3.1.
4. While the version is `0.x`, the bump follows the public API change. A
   release whose public API changes are not strictly additive bumps the minor
   version (`0.x.y` to `0.(x+1).0`): anything removed, renamed or changed in
   signature or type, behaviour a caller can observe changing, or a variant or
   field added to an exhaustive public enum or struct. A release whose changes
   are strictly additive, or change no public API, bumps the patch version
   (`0.x.y` to `0.x.(y+1)`). This applies from the version at which a project
   adopts these rules; earlier versions are not re-labelled.
5. The public API rule is recursive through dependencies. When a project with
   a public API moves a dependency to a version that is not semver-compatible
   with the one it used (for a `0.x` dependency, a new minor version; from
   1.0, a new major version), its own release is not strictly additive and
   bumps its minor version under Rule 5.4 (its major version from 1.0), even
   when its own code does not change. Every project with a public API that
   depends on it does the same when it takes that release, and so on up the
   chain. The `VERSIONS.md` entry names the dependency with its old and new
   versions and marks the change **Breaking**. A repository with no public
   API (an application) only updates its manifest and lock file.

## Repowiki

Project documentation is browsed with repowiki (`~/src/repowiki`), a static
viewer that collects each registered project's README, `docs/` and plans.
To register a new project:

1. Add `repowiki.toml` at the project root:

   ```toml
   # repowiki.toml
   # Read by repowiki's collector from the repository root.

   # Landing page of the registered site.
   default_route = "/README"

   # Symlinked as static files, not listed in navigation. out/ is gitignored
   # render output; its images resolve as ./out/... and stay unlisted.
   static_dirs = ["out"]

   [crate_resolution]
   # Only these folders become navigation groups; docs/design/*.md is grouped
   # as a "design" package, one entry per plan.
   prefixes = ["docs"]
   ```

   Add the folders of a workspace's other crates to `prefixes` (for example
   `"crates"`).
2. Append the project to `~/src/repowiki/sites.toml`:

   ```toml
   [[site]]
   source = "/Users/tmeade/src/<project>"
   ```

   The site id defaults to the source's parent and own folder joined by
   `--`; set `id` only when that would collide.
3. Run `make serve` in `~/src/repowiki` to collect and view it.

Documents link evidence images as `./out/<area>/<YYYYMMDD_NN_name>/...`,
relative to the repository root, so they render in repowiki from the
gitignored `out/` directory.

## Voice agents

Voice agents (such as flankr's general agent) read this file to talk about
the project, and their replies are spoken.

1. Say a plan or document by the words of its name, without its date,
   sequence number or path:
   `docs/design/20261027_04_GENERAL_AGENT_PLANNING_PLAN.md` is "the general
   agent planning plan".
2. Leave out commit hashes, line numbers and timestamps unless asked for
   them.

---

# This project: libmorph

Rust, one Cargo workspace. The `libmorph` library is at the repository root
(`src/`, `build.rs`, and a `Cargo.toml` that is also the workspace
manifest), with `crates/libmorph-build` beside it. Examples are in
`examples/`, with Android example crates under `examples/android/` and
script files under `examples/scripts/`; integration tests are in `tests/`,
helper scripts in `scripts/`, and specs and plans in `docs/design/`.
Generated evidence, scripted runs' screenshots included, goes under the
gitignored `out/`.

## Packages for Rule 3.1

`Cargo.toml`'s `[workspace] members`. Each one is a package, so at most one
of them per commit:

| Path | Package |
| --- | --- |
| `.` | `libmorph` |
| `crates/libmorph-build` | `libmorph-build` |
| `examples/android/audiocapture` | `libmorph-example-audiocapture-android` |
| `examples/android/securestore` | `libmorph-example-securestore-android` |
| `examples/android/urlopen` | `libmorph-example-urlopen-android` |
| `examples/android/background` | `libmorph-example-background-android` |

The root `Cargo.toml` is both `libmorph`'s manifest and the workspace
manifest. A change to its `[package]`, `[features]`, `[dependencies]` or
`[[example]]` tables is part of the `libmorph` commit; a change to its
`[workspace]` table is repository-level and goes in its own commit, except
when it adds a package in that package's first commit.

## Commands for Rule 3.4

| What | Command |
| --- | --- |
| Formatter | `cargo fmt -p <crate> -- --check` |
| Linter | `cargo clippy -p <crate> --all-targets`, no new warnings |
| Tests | `cargo test -p <crate>` |

The `script` feature gates a large amount of code - the script runner, its
offscreen target and PNG capture, and the tests that drive them - which the
default features do not build. So the linter and the tests are run again
with it:

```sh
cargo clippy -p libmorph --all-targets --features script
cargo test -p libmorph --features script
```

That is how this crate has in fact been verified. A commit that touches the
runner, a script verb, or anything an integration test drives is not
checked by the default-feature run alone.

## Versions and tags for Rule 5

Two crates here have public APIs and are versioned on their own, each by
`version` in its own `Cargo.toml`:

| Crate | Tag |
| --- | --- |
| `libmorph` | `v<version>` |
| `crates/libmorph-build` | `libmorph-build-v<version>` |

`VERSIONS.md` records both, one entry per version change, oldest first,
headed `## <crate> <version> — YYYY-MM-DD (<short commit>, tag <tag>)`: the
crate is named because the file carries two of them. An entry written in
the version commit itself names only the tag, since a commit cannot contain
its own hash. The Android example crates are not released and get no
entries.

libmorph re-exports libdevice, libmobile's types, wgpu and cpal, so Rule
5.5 reaches it through all four: an incompatible bump in any of them is not
strictly additive here either.

## Conventions for whoever edits the code

- **libmorph is a wrapper.** A new platform capability is planned, built
  and released in libmobile (Android) or libdevice (the desktop) first,
  with an example there that genuinely uses it; libmorph is then amended to
  wrap it. Where both crates provide a capability they expose the same API,
  and libmorph wraps both. libmorph does not reach past them to the NDK,
  `android-activity` or OS frameworks. What it owns is what it exists for:
  wgpu and GPU bring-up, the winit event loop and window with device
  emulation and input, cpal off Android, and scripted runs. A new exception
  is agreed with the owner first.
- **Ground truth over any document.** Before using a libdevice or libmobile
  API, check it against the source in `~/src/libdevice` and
  `~/src/libmobile`. Where the source disagrees with a document, this file
  included, the source wins; note the discrepancy.
- **Feature gating.** Gate a subsystem's code with `#[cfg(feature = "…")]`
  and give it zero-sized stubs, so `MorphContext`'s signatures are the same
  with the feature and without it.
- **`cfg(script_runner)`.** `build.rs` sets it when `script` is on and the
  target is neither Android nor wasm32. Gate runner code on
  `#[cfg(script_runner)]` rather than repeating that condition. The runner
  module also carries `compile_error!` guards: `web` is required on wasm32,
  `mobile` on Android.
- **Lifecycle hazards.** Never hold a surface past `WindowDestroyed` or
  `suspended`. Never block the wasm main thread.
- **Passthrough features.** Each `mobile-*` passthrough also enables
  `mobile`, so `libmobile/<x>` is never asked for without the dependency
  itself. Add a passthrough only once the upstream feature exists.
- **Android dependencies.** `android-activity` and `ndk` arrive through
  libmobile's re-exports; declared separately they duplicate the JavaVM
  singleton. `winit` is a dependency of `cfg(not(target_os = "android"))`
  only.
- **Local checkouts.** Build against sibling checkouts with an
  uncommitted `.cargo/config.toml` (it is gitignored) that patches the
  libdevice and libmobile git URLs to paths. Never commit a `path`
  dependency, or a `Cargo.lock` resolved with a patch active: the patch
  strips that entry's `source = "git+…"` line.
