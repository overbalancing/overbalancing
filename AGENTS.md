# Project

Overpalancing is a Rust and Bevy puzzle game inspired by Swing (1997).
The repository directory and GitHub organization are named `overbalancing`.
Use English for code, UI, documentation, commits and PRs; speak German to the customer.
All authored specifications, ADRs, role guides and documentation use HTML.
AGENTS.md, the short README and GitHub PR templates are harness/discovery exceptions.

# ProjectAtlas

Use the installed, version-matched ProjectAtlas skill and MCP tools before broad source reads.
Start with `atlas_session_brief` and this checkout's explicit `project_path`.
If MCP is unavailable, use `projectatlas overview`, then bounded folders/files and exact slices.
If an MCP request stalls, stop the request and use the CLI fallback; do not leave subagents blocked indefinitely.
Run init only if project-local state is absent or setup verification is explicitly requested.
Refresh changed files with `atlas_watch_once` / `projectatlas watch --once`.
Full scans require an absent index or typed recovery guidance. Keep local Atlas state ignored.
Every worktree owns its own index; never share writable databases.

# Roles and communication

The lead owns planning, spec routing, delegation, integration, review and customer communication.
Delegate feature implementation to implementers; the lead may make integration fixes.
The architect owns boundaries and proposed ADRs, evaluates crates and alternatives, and reviews structural changes.
Implementers deliver a complete feature against its spec and report only urgent blockers or a concise final result to the lead.
Report stalled tools or required permission immediately to the lead; continue independent work instead of waiting indefinitely.
The UX engineer owns interaction design, accessibility, layout and UI acceptance review.
Role instructions: `agents/lead.html`, `agents/architect.html`, `agents/implementer.html`, `agents/ux-engineer.html`.
Use isolated worktrees for overlapping implementations; bounded disjoint file ownership is allowed for coordinated setup.
Do not ask the customer to coordinate subagents. Batch material questions through the lead.
Every customer question must carry a unique identifier, for example `0003-CRATE-SELECTION`.
Track questions, answers and status in `docs/questions.html`.

# Decisions and dependencies

The customer chooses technologies. Rust and Bevy are selected; new direct dependencies require customer confirmation.
Before proposing a crate, the architect checks license, bus-factor evidence, recent releases/commits,
reputation/security signals and alternatives, with dated primary source links. Never invent a bus factor.
ADRs live in root `adr/`. Proposed decisions remain `proposed` until the customer merges their approval PR.
Only the customer may merge a PR that transitions any ADR to `accepted` (canonical) or `approved` (legacy synonym).
Agents may merge docs, CI and chore PRs after required checks pass, provided they do not approve an ADR.
Feature/story PRs require customer acceptance; a chore label does not authorize merging feature changes.

# Delivery

Use an OpenSpec-inspired HTML workflow in `specs/`: canonical capability specs, active changes with
proposal/design/tasks, then archive delivered changes. Route new customer ideas to existing specs or create a new one.
Preserve unresolved rules explicitly; do not silently invent game mechanics.
One feature/story PR contains the entire usable feature. Avoid intermediate plumbing PRs.
Clean the branch before PR creation: 1–6 focused Conventional Commits, no fixup/WIP commits or unrelated edits.
Never rewrite shared history without coordination. PRs describe visible behavior, review guide, dependencies,
validation and remaining platform proof. Do not claim device runs from cross-compilation.

# Engineering and verification

Keep deterministic game rules in `overpalancing-core`, with no Bevy, I/O or platform dependencies.
Keep rendering, input and platform entry points in `overpalancing-app`.
Prefer functions and concrete data over speculative abstractions; add no physics engine without a reviewed need.
No copied reference-project code or assets. Core forbids unsafe code; platform exceptions require an ADR.
Use stable Rust pinned in rust-toolchain.toml; MSRV is checked only in scheduled nightly CI, not every PR.
Before delivery run fmt, clippy with warnings denied, nextest, doctests, audit, deny, rustdoc and HTML checks.
Run meaningful domain tests; verify UI with a real render smoke when available.
CI builds Linux, Windows, macOS, Android and iOS. Signing and store releases require separate explicit authorization.
For assets, ask the customer for asset type, exact dimensions, format and purpose before adding placeholders requiring art.
License/commercial rights and store distribution are documented in LICENSE.html; third-party notices retain their own terms.
