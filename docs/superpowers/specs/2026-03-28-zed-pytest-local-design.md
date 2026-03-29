# zed-pytest-local Extension — Design Spec

**Date:** 2026-03-28
**Status:** Approved
**Repo:** `git@github.com:SLR-Software-Solutions-Inc/zed-pytest-local.git`

---

## Problem

pytest has no native concept of a local override config file. Once `pytest.ini` is committed, all developers share the same settings. Separately, Zed IDE does not automatically detect a project's `.venv` and configure it as the Python interpreter. Both require per-developer manual setup that is error-prone and not portable.

---

## Goals

1. When `pytest_local.ini` (configurable name) is present in a project, pytest must automatically use it to override `pytest.ini` — for every invocation path: gutter run/debug buttons, task runner, terminal CLI. This is achieved via a committed `conftest.py` plugin that pytest loads on every run, regardless of how it was invoked. The Zed extension cannot intercept gutter invocations directly (see Non-Goals); `conftest.py` is the mechanism that makes gutter coverage work.
2. When `.venv` or `venv` (configurable list) is present, Zed must use it as the Python interpreter for all LSPs (ty, pyright, basedpyright). This is achieved by the slash command writing interpreter paths into `.zed/settings.json`.
3. Both behaviors must be configurable per-project via `.zed/settings.json`.
4. The extension must load correctly in Zed's extension system.

---

## Non-Goals

- Automatic setup on workspace open (not possible — Zed extension API has no `on_workspace_opened` hook)
- Intercepting Zed's gutter runnable commands directly (not possible — runnables are Tree-sitter only)
- Global (cross-project) settings (all config is per-project via `.zed/settings.json`)

---

## Architecture

Two layers with distinct responsibilities:

```
┌─────────────────────────────────────────────────┐
│  SETUP LAYER (Zed extension — runs once)        │
│  /pytest_local slash command                    │
│  • reads .zed/settings.json config              │
│  • detects venv, writes LSP interpreter paths   │
│  • injects conftest.py plugin                   │
│  • creates pytest_local.ini template            │
│  • updates .gitignore                           │
└─────────────────────────────────────────────────┘
                       ↓ writes
┌─────────────────────────────────────────────────┐
│  RUNTIME LAYER (conftest.py — runs every test)  │
│  • reads .zed/settings.json → get ini_filename  │
│  • loads pytest_local.ini from rootdir          │
│  • applies [pytest] section overrides           │
│  • applies [pytest_local] convenience toggles   │
│  Works for: gutter ▶, debug 🐛, CLI, CI        │
└─────────────────────────────────────────────────┘
```

---

## File Structure

```
zed-pytest-local/
├── extension.toml              # Zed manifest — slash command, schema_version = 1
├── Cargo.toml                  # zed_extension_api (pinned to correct version)
├── src/
│   └── lib.rs                  # Slash command handler (Rust/WASM)
├── templates/
│   ├── conftest_snippet.py     # pytest plugin code (injected into projects)
│   └── pytest_local.ini        # template local config (gitignored per project)
└── docs/
    └── superpowers/specs/      # this file
```

---

## Configuration Schema

All configuration lives in the project's `.zed/settings.json` under the `pytest_local` key:

```json
{
  "pytest_local": {
    "ini_filename": "pytest_local.ini",
    "venv_dirs": [".venv", "venv"]
  }
}
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `ini_filename` | string | `"pytest_local.ini"` | Name of the local override ini file |
| `venv_dirs` | string[] | `[".venv", "venv"]` | Venv directory names to search, first match wins |

The slash command merges these defaults with existing settings — it never overwrites keys already present.

---

## Slash Command: `/pytest_local`

### Execution Steps (idempotent — safe to re-run)

1. **Read config** — if `.zed/settings.json` is absent, treat as empty JSON object and create it (plus the `.zed/` dir) during step 3/4. Extract `pytest_local.ini_filename` and `pytest_local.venv_dirs`; apply defaults for absent keys.
2. **Detect venv** — iterate `venv_dirs` in order; pick first directory that exists in project root. If none found: log `⚠ no venv detected — skipping interpreter config` and skip step 3; continue all other steps.
3. **Write interpreter** — merge detected venv Python path into `.zed/settings.json` under `lsp.ty`, `lsp.pyright`, `lsp.basedpyright`; never touch unrelated keys.
4. **Write `pytest_local` defaults** — ensure `pytest_local.*` block exists in `.zed/settings.json` with defaults for any missing keys.
5. **Inject conftest.py** — check for `# BEGIN pytest-local-ini` guard; skip if already present. If `conftest.py` exists and already defines `pytest_configure` (without the guard): append the block with a comment warning the user to check for duplicate `pytest_configure` definitions and merge manually if needed. Create file if absent.
6. **Create ini template** — create `<ini_filename>` from template if not present; skip if already exists.
7. **Update .gitignore** — add `<ini_filename>` entry if absent.
8. **Report** — output ✓/✗/⚠ status for each step.

**Note on partial failure:** The command writes files sequentially with no rollback. Each step is independently idempotent — re-running the command after a partial failure is safe and will complete remaining steps.

### `.zed/settings.json` written by the command

```json
{
  "pytest_local": {
    "ini_filename": "pytest_local.ini",
    "venv_dirs": [".venv", "venv"]
  },
  "lsp": {
    // ty: pythonPath key needs verification against ty docs before writing
    // omit ty block if key cannot be confirmed; ty may auto-detect venv
    "pyright": {
      "settings": {
        "python": { "pythonPath": ".venv/bin/python", "venvPath": ".", "venv": ".venv" }
      }
    },
    "basedpyright": {
      "settings": {
        "python": { "pythonPath": ".venv/bin/python", "venvPath": ".", "venv": ".venv" }
      }
    }
  }
}
```

---

## Runtime Plugin: conftest.py

The plugin is injected once by the slash command and committed to the project repo.

### pytest_configure hook flow

```
pytest_configure(config)
  → resolve rootdir via Path(str(config.rootdir))
  → look for .zed/settings.json relative to rootdir
  → read ini_filename from settings (default: "pytest_local.ini")
  → look for <ini_filename> in rootdir
  → if absent: return silently
  → parse ini with configparser
  → [pytest] section: apply each key via _apply_ini_override()
  → [pytest_local] section:
      enabled = true → apply log_cli=true, log_cli_level=DEBUG
  → done
```

### Multi-version Compatibility (`_apply_ini_override`)

Exclusive try/except chain (only first successful method runs):

| Priority | Method | Covers |
|----------|--------|--------|
| 1 | `ConfigValue(origin="override")` via `_inicfg` | pytest 9+ |
| 2 | `config.override_ini(key, value)` | pytest 6–8 |
| 3 | `config._inicache[key] = value` | last resort |

Verified working against pytest 9.0.2 (Python 3.14.3) in the test project.

### pytest_local.ini template (created by slash command)

```ini
[pytest]
addopts = -v --tb=long

[pytest_local]
# enabled = true → auto-enables log_cli + log_cli_level=DEBUG
enabled = true
```

---

## Extension Loading Fix

### Root cause
`zed_extension_api` version in `Cargo.toml` must be compatible with what the installed Zed binary expects. Mismatch causes silent load failure.

### Fix
- Query crates.io for the latest stable `zed_extension_api` version and confirm it against the Zed release notes or changelog
- Pin `Cargo.toml` to that version
- Verify `extension.toml`:
  - `schema_version = 1` (verify this is still current for the installed Zed — check `zed: open log` after loading if in doubt)
  - `id` and `name` use only alphanumeric + hyphens
  - `[slash_commands.<name>]` key uses underscores not hyphens
- Confirm WASM target is `wasm32-wasip1` (not the deprecated `wasm32-wasi`)
- Confirm `crate-type = ["cdylib"]` in `Cargo.toml`
- **Note:** `lsp.ty` settings schema (`python.pythonPath`) must be verified against `ty`'s documentation — `ty` is a new Astral tool and may use a different key than pyright

### Dev extension install process (Zed)
1. `Cmd+Shift+X` → Extensions panel
2. Click **"Install Dev Extension"**
3. Select `/Users/narender/dev/zed-pytest-local`
4. Zed compiles and loads — check `zed: open log` for errors

---

## Precedence

```
CLI args  >  pytest_local.ini  >  pytest.ini / pyproject.toml / setup.cfg / tox.ini
```

`pytest_local.ini` overrides whichever config file pytest discovered as its primary ini. The `conftest.py` plugin fires after pytest's own ini-file discovery, so it works regardless of whether the project uses `pytest.ini`, `pyproject.toml`, or `setup.cfg`.

---

## What Does NOT Change

- `conftest.py` is committed to the project repo (it's project infrastructure, not personal config)
- `pytest_local.ini` is always gitignored (it's personal config)
- `pytest.ini` is untouched
- Zed's global `settings.json` (`~/.config/zed/settings.json`) is never written — only `.zed/settings.json` (project-local)

---

## Verification

1. Install dev extension via Extensions panel
2. Open `test_project/` in Zed
3. Run `/pytest_local` slash command
4. Verify `.zed/settings.json` has `pytest_local.*` + `lsp.*` blocks
5. Verify `conftest.py` has `# BEGIN pytest-local-ini` block
6. Verify `pytest_local.ini` created and in `.gitignore`
7. Run `pytest` in terminal → confirm DEBUG logs + `-v` active
8. Click gutter ▶ on a test → confirm same output
9. Rename ini: change `ini_filename` in `.zed/settings.json`, rename file, re-run tests → confirm new name picked up
10. Change venv: set `venv_dirs: ["myenv"]`, create `myenv/`, re-run `/pytest_local` → confirm new interpreter path written
