# pytest-local — Zed Extension

Override `pytest.ini` locally without committing changes. Per-developer settings in a gitignored `pytest_local.ini` — works across every pytest invocation path in Zed: gutter ▶ buttons, debugger, task runner, and terminal CLI.

```
Precedence (highest → lowest):
  CLI args  >  pytest_local.ini  >  pytest.ini
```

---

## How It Works

On every Python file open, the extension:

1. Detects `.venv` or `venv` in the project root
2. Installs a pytest plugin into `.venv/site-packages/` via a `pytest11` entry-point
3. Creates `pytest_local.ini` (from `pytest.ini` as template, or built-in template)
4. Adds `pytest_local.ini` to `.gitignore`
5. Writes Python interpreter paths to `.zed/settings.json` for LSP and terminal

The plugin auto-loads on **every pytest run** — no conftest.py changes, no committed project files touched.

---

## Requirements

- A `.venv` or `venv` directory in the project root with Python installed
- Zed with the extension installed (see Setup below)

---

## Setup

### 1. Install the extension

**From Zed marketplace** (once published):
- Open Zed → `Cmd+Shift+X` (Extensions) → search `pytest-local` → Install

**As a dev extension** (for now):
- Clone this repo
- Open Zed → Extensions → **Install Dev Extension** → select the cloned directory

### 2. Add `pytest-local` to your Python language servers

This is a one-time change to your Zed settings (`~/.config/zed/settings.json`).

Find the `languages.Python.language_servers` array and add `"pytest-local"`:

```json
{
  "languages": {
    "Python": {
      "language_servers": ["pytest-local", "..."]
    }
  }
}
```

The `"..."` keeps your existing language servers (pyright, ty, etc.). If you already have an explicit list, just insert `"pytest-local"`:

```json
"language_servers": ["ty", "pytest-local", "!pyright", "!basedpyright"]
```

> **Why is this needed?** Zed requires extension language servers to be explicitly opted in when you have a custom `language_servers` list. If you have no `language_servers` setting, the extension activates automatically with no configuration needed.

### 3. Trust the project

The first time you open a project in Zed from outside your home directory (e.g. `Downloads/`), Zed will show a **"Trust this project?"** banner. Click **Trust** — language servers (including ours) won't start until the project is trusted.

### 4. Open a Python file

Open any `.py` file in your project. The extension activates automatically and:
- Installs the pytest plugin to `.venv/site-packages/`
- Creates `pytest_local.ini` in the project root
- Adds `pytest_local.ini` to `.gitignore`
- Creates/updates `.zed/settings.json` with interpreter paths

---

## pytest_local.ini Reference

```ini
# pytest_local.ini — local overrides (gitignored, per-developer)
# Edit freely. Never commit this file.

[pytest]
# Standard pytest ini options — override pytest.ini
addopts = -v --tb=long
log_cli = true
log_cli_level = DEBUG
testpaths = tests

[pytest_local]
# enabled = true  →  shorthand for log_cli=true + log_cli_level=DEBUG
enabled = true
```

All standard pytest ini-options are supported in `[pytest]`. The `[pytest_local]` section provides convenience toggles:

| Key | Effect |
|-----|--------|
| `enabled = true` | Enables `log_cli=true` + `log_cli_level=DEBUG` |

---

## Verifying It Works

After opening a Python file, check:

1. **`pytest_local.ini` exists** in the project root
2. **`.gitignore`** contains `pytest_local.ini`
3. **`.zed/settings.json`** was created with `lsp` and `terminal` entries
4. **pytest session header** shows:
   ```
   local config: /path/to/pytest_local.ini (3 overrides applied)
   ```

If the header shows `local config: NOT FOUND (...)`, the extension ran but `pytest_local.ini` doesn't exist yet.
If `local config: inactive`, the plugin loaded but no ini file was found.

---

## Invocation Paths

All paths work automatically once the plugin is installed to `.venv/site-packages/`:

| Path | How it works |
|------|-------------|
| **Gutter ▶ button** | Zed launches pytest → plugin auto-loads from site-packages → ini applied |
| **Debugger (F5/F4)** | Same as gutter — debugpy invokes pytest, plugin fires |
| **Task runner** | Same — pytest always loads installed plugins |
| **Terminal `pytest`** | Same — entry-point is discovered automatically |

---

## Files Created by the Extension

| File | Committed? | Purpose |
|------|-----------|---------|
| `pytest_local.ini` | **No** (gitignored) | Your personal pytest overrides |
| `.zed/settings.json` | Optional | LSP interpreter paths, terminal PATH |

No `conftest.py` is touched. No `pytest.ini` is modified.

---

## Troubleshooting

**Extension never activates / `.zed/` not created**

1. Check `language_servers` in `~/.config/zed/settings.json` — `"pytest-local"` must be present
2. Check the project is **trusted** in Zed (command palette → `trust workspace`)
3. Check Zed logs (`~/Library/Logs/Zed/Zed.log`) for:
   ```
   starting language server process... pytest-local
   ```
   If absent, one of the above is the cause.

**Plugin not loading in pytest**

Run pytest manually and check the plugins line:
```
plugins: local-ini-0.1.0, ...
```
If `local-ini-0.1.0` is missing, the plugin wasn't installed. Open a Python file in the project in Zed to trigger installation, then retry.

**Wrong Python used for gutter play**

The extension writes `.zed/settings.json` with the venv Python path. If the wrong Python is used:
- Confirm `.zed/settings.json` exists and has `terminal.env.PATH` pointing to `.venv/bin`
- Restart Zed to pick up the new settings

**`pytest_local.ini` settings not taking effect**

- Confirm the file has a `[pytest]` section header
- Run `pytest -v` and look for `local config: /path (N overrides applied)` in the header
- If `0 overrides applied`, your keys may be misspelled or unsupported ini-options

---

## Contributing

Issues and PRs welcome at the project repository.
