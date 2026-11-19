# Voxcell Desktop — Build & Test

How to build and test the `voxcell-desktop` Rust workspace (7 crates) on the RUNE
framework, and how to regenerate the cross-language golden vectors
(spec: `docs/RUNE_DESKTOP_SPEC.md` §5/§9, tickets P0-01 / P0-04).

All commands below assume the repository root (`hermes-agentis/`) unless noted.
The Rust workspace itself lives in `voxcell-desktop/`.

## 1. Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust (stable) | 1.99+ | `rustup toolchain install stable` |
| Git | any recent | submodule support required |
| Node.js | 22+ (verified on 24) | only for golden-vector regeneration |
| tsx | 4.21+ | already in the web `node_modules` (`npm install` / `bun install` at repo root), or fetched on demand via `npx --yes tsx@4.21.0` |

### RUNE submodule (required before any cargo command)

RUNE is pinned as a git submodule at `third_party/rune`
(`anomalyco/rune` @ `f54cbcfa84d995d339c243a579218f5062c940b5`). The workspace
`Cargo.toml` mirrors RUNE's vendored `[patch.crates-io]` entries
(`wgpu-hal`, `winit`, `sctk-adwaita`, `parley_engine`, `accesskit_*`) via paths
into `third_party/rune/vendor/`, so the submodule **must** be checked out:

```bash
git submodule update --init --recursive
```

Fresh clone shortcut: `git clone --recurse-submodules <repo-url>`.

### Linux system packages

Building the workspace on Linux needs (Debian/Ubuntu names):

```bash
sudo apt-get install -y pkg-config libasound2-dev libclang-dev \
  libfontconfig1-dev libxkbcommon-dev libvulkan1 mesa-vulkan-drivers
```

- `libasound2-dev` + `pkg-config` — `alsa-sys` (cpal audio backend)
- `libclang-dev` — bindgen/clang-sys build scripts
- `libfontconfig1-dev` — fontique/fontconfig text backend
- `libxkbcommon-dev` — winit keyboard handling
- `libvulkan1` + `mesa-vulkan-drivers` — software Vulkan (lavapipe) for headless
  GPU tests on machines without a real GPU

X11/Wayland client libraries are `dlopen`'d by the vendored winit and are **not**
needed for headless `cargo test`. macOS and Windows need no extra packages.

## 2. Build & test the workspace

Run from `voxcell-desktop/`:

```bash
cd voxcell-desktop

# Compile check (fast, no codegen)
cargo check --workspace --locked

# Full headless test suite
cargo test --workspace --locked

# Single crate (preferred while iterating)
cargo check -p voxcell-core
cargo test  -p voxcell-core
```

Notes:

- **Headless:** RUNE's headless `Runtime` renders offscreen; tests need no
  display server, no `DISPLAY`, and no windowing system. This is what CI runs.
- **Framework boundary:** `voxcell-core`, `voxcell-hermes`, `voxcell-halftone`,
  `voxcell-audio`, `voxcell-store` are framework-free (no `rune`/`wgpu` imports).
  RUNE lives only in `voxcell-ui` / `voxcell-app`.
- **Shared build lock:** several agents/terminals may build concurrently;
  `Blocking waiting for file lock on build directory` is normal — wait it out.
- **`--locked`:** `Cargo.lock` is committed; CI builds with `--locked`. If you
  intentionally change dependencies, commit the updated `Cargo.lock` too.
- Tests use inline fixtures/mocks only — no live network calls, no real API keys.

## 3. Golden vectors (`tests/vectors/`)

Deterministic JSON fixtures generated from the **pure web reference utilities**
(`src/utils/*.ts`, read-only) lock Rust ports against TypeScript behavior
(spec §9.1). Consumed by `cargo test -p voxcell-core -p voxcell-hermes`.

### Regenerate

From the **repo root**:

```bash
npx tsx voxcell-desktop/tools/gen-vectors.mjs
```

(If the web `node_modules` are not installed: `npx --yes tsx@4.21.0 voxcell-desktop/tools/gen-vectors.mjs` —
the generator only imports local TS files, so standalone tsx is sufficient.)

Output lands in `voxcell-desktop/tests/vectors/<module>/<function>.json` plus
`manifest.json`. Current coverage (7 modules, 0 skipped):

| Module | Files | Cases |
|---|---|---|
| hermesProtocol | 21 | 106 |
| profileRegistry | 8 | 46 |
| settingsSnapshot | 9 | 38 |
| responsiveLayout | 3 | 51 |
| taskInsights | 23 | 109 |
| sidebarCharacterState | 2 | 20 |
| threadTrace | 4 | 27 |

### Vector file schema

```json
{
  "module": "responsiveLayout",
  "function": "resolveThreadTasksLayoutMode",
  "description": "…",
  "cases": [ { "name": "w=900,lg=true", "input": { "stageWidthPx": 900, "isDesktopLg": true }, "expected": "split" } ]
}
```

- `input` is a named-argument object, JSON-native. One exception:
  `mergeTasksWithSessionActivity.confirmedTaskIds` is an **array** in JSON
  (the TS API takes a `ReadonlySet`).
- Stream-parser vectors (`SseLineDecoder`, `ChatCompletionsStreamParser`,
  `SessionChatStreamParser`, `RunsEventStreamParser`) use
  `input.chunks: string[]` (wire chunks fed to `push`) and
  `expected: { result, events }` where `events` are the captured callback calls.
- `fetchWithConcurrentRetry` vectors declare mock `responses` + `opts`; the
  generator injects `makeRequest`/`sleep` — no network is ever touched.

### Determinism contract

The generator pins, **before importing any web module**:

- `TZ=UTC` (required: `formatStartClock` uses local-time getters — vectors are
  only valid under UTC; CI sets this via the generator itself)
- `Date.now()` → `1795089600000` (`2026-11-19T12:00:00.000Z`)
- `Math.random` → seeded mulberry32 (`0x5eed1234`)
- `crypto.randomUUID` → counter-based UUIDv4-shaped shim (only affects the
  `createIdempotencyKey` suffix; prefix/sanitization logic under test is real)

JSON is canonicalized: object keys sorted recursively, array order preserved,
`-0` → `0`, 2-space indent, trailing newline. Two consecutive runs produce
byte-identical files (verified via SHA-256).

Browser-gated functions (localStorage/DOM helpers inside `uiCustomization` /
`backgrounds/persistence`) are never invoked — only pure exports are exercised,
which is why all 7 modules import cleanly under headless Node. If a future
module gains a browser-only import-time dependency, the generator skips it and
records the reason in `manifest.json` (`modules[].status = "skipped"`).

### Using vectors in Rust tests

Parse with `serde_json::Value` (or typed structs) and compare against the Rust
implementation's output. Because keys are canonically sorted, either compare
parsed values (order-insensitive) or serialize with sorted keys
(`serde_json` + `BTreeMap`, or the `preserve_order` feature off). Remember the
UTC assumption for any `formatStartClock`-derived fields.

### Drift protection

CI job `vectors` (`.github/workflows/rune-desktop.yml`) regenerates the vectors
on every change to `src/utils/**`, `src/types/**`, `src/backgrounds/**`, or
`voxcell-desktop/**` and fails on `git diff --exit-code -- voxcell-desktop/tests/vectors`.
**If you change a web util's behavior, regenerate and commit the vectors in the
same change** (spec §10 drift mitigation).

## 4. CI

`.github/workflows/rune-desktop.yml`:

- **check-test** matrix on `ubuntu-latest` + `windows-latest`:
  checkout with `submodules: recursive` → Linux apt deps → Rust stable →
  `cargo check --workspace --locked` → `cargo test --workspace --locked` (headless).
- **vectors** on `ubuntu-latest`: Node 24 + standalone tsx regeneration + drift check.

Triggers: pushes to `rune-desktop`/`main` and PRs touching the workspace, the
submodule pointer, the workflow, or the web vector sources; plus manual
`workflow_dispatch`.

## 5. Troubleshooting

- **`failed to load manifest for dependency … /tmp/rune-ref`** — historical
  scaffold drift; the workspace now uses the `third_party/rune` submodule.
  Run `git submodule update --init --recursive`.
- **`package not found` in vendor patches** — the submodule checkout is partial;
  re-run submodule update with `--recursive` and verify
  `third_party/rune/vendor/` contains `wgpu-hal`, `winit`, `parley_engine`, etc.
- **Linux link errors (`alsa`, `fontconfig`, `clang`)** — install the packages
  from §1.
- **Vector mismatch after editing web utils** — expected; regenerate (§3) and
  commit, or fix the regression. Never hand-edit generated vector files.
- **`Blocking waiting for file lock`** — another cargo process holds the shared
  target dir; wait.
