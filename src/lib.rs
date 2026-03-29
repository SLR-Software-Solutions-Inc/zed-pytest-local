use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

// ── Pytest plugin installed into .venv/site-packages/ ─────────────────────────
// Loaded automatically by pytest via the pytest11 entry-point — no conftest.py needed.
const PLUGIN_CODE: &str = r##"import configparser
import json
from pathlib import Path

# Stores state set in pytest_configure, read by pytest_report_header
_state = {"ini_path": None, "overrides": 0, "missing": False}


def _apply_ini_override(config, key, value):
    """Apply a single ini override — compatible with pytest 6, 7, 8, 9+."""
    try:
        from _pytest.config import ConfigValue  # type: ignore[attr-defined]
        if hasattr(config, '_inicfg') and isinstance(config._inicfg, dict):
            config._inicfg[key] = ConfigValue(value=value, origin="override", mode="ini")
            if hasattr(config, '_inicache'):
                config._inicache.pop(key, None)
            return
    except (ImportError, Exception):
        pass
    if hasattr(config, 'override_ini'):
        try:
            config.override_ini(key, value)
            return
        except Exception:
            pass
    try:
        if hasattr(config, '_inicache'):
            config._inicache[key] = value
    except Exception:
        pass


def pytest_configure(config):
    """Read pytest_local.ini and override active pytest settings.

    ini filename is read from .zed/settings.json (key: pytest_local.ini_filename),
    defaulting to pytest_local.ini. File is gitignored — per-developer local overrides.

    Sections:
        [pytest]       — standard pytest ini options (override pytest.ini)
        [pytest_local] — enabled = true  →  log_cli=true + log_cli_level=DEBUG
    """
    try:
        rootdir = Path(str(config.rootdir))
    except AttributeError:
        return

    ini_filename = "pytest_local.ini"
    settings_path = rootdir / ".zed" / "settings.json"
    if settings_path.exists():
        try:
            with open(settings_path) as f:
                settings = json.load(f)
            ini_filename = settings.get("pytest_local", {}).get("ini_filename", ini_filename)
        except Exception:
            pass

    local_ini = rootdir / ini_filename
    if not local_ini.exists():
        _state["missing"] = True
        _state["ini_path"] = str(local_ini)
        return

    parser = configparser.ConfigParser()
    parser.read(local_ini)
    count = 0

    if parser.has_section("pytest"):
        for key, value in parser["pytest"].items():
            _apply_ini_override(config, key, value)
            count += 1

    if parser.has_section("pytest_local"):
        if parser.get("pytest_local", "enabled", fallback="").strip().lower() == "true":
            _apply_ini_override(config, "log_cli", "true")
            _apply_ini_override(config, "log_cli_level", "DEBUG")
            count += 2

    _state["ini_path"] = str(local_ini)
    _state["overrides"] = count


def pytest_report_header(config):
    """Show local config status in the pytest session header."""
    if _state["missing"]:
        return f"local config: NOT FOUND ({_state['ini_path']}) — create it to override pytest.ini"
    if _state["ini_path"]:
        return f"local config: {_state['ini_path']} ({_state['overrides']} overrides applied)"
    return "local config: inactive"
"##;

// ── No-op LSP: stays alive, responds to lifecycle messages, provides no features ──
// Zed requires a running process; this satisfies the protocol without interfering
// with pyright / ty / basedpyright.
const NOOP_LSP: &str = r##"import json,sys
def r():
    h=b""
    while not h.endswith(b"\r\n\r\n"):h+=sys.stdin.buffer.read(1)
    n=int(h.decode().split("Content-Length: ")[1].split("\r\n")[0])
    return json.loads(sys.stdin.buffer.read(n))
def w(m):
    b=json.dumps(m).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode()+b)
    sys.stdout.buffer.flush()
while True:
    m=r()
    if "id" in m:
        if m.get("method")=="initialize":w({"jsonrpc":"2.0","id":m["id"],"result":{"capabilities":{}}})
        elif m.get("method")=="shutdown":w({"jsonrpc":"2.0","id":m["id"],"result":None})
        else:w({"jsonrpc":"2.0","id":m["id"],"result":None})
    elif m.get("method")=="exit":break
"##;

const DEFAULT_INI_TEMPLATE: &str = r#"# pytest_local.ini — local overrides (gitignored, per-developer)
# Edit freely. Never commit this file.
#
# [pytest]  section: overrides pytest.ini settings
# [pytest_local] section: extension convenience options

[pytest]
# addopts = -v --tb=long
# log_cli = true
# log_cli_level = DEBUG

[pytest_local]
# enabled = true  →  auto-adds log_cli=true + log_cli_level=DEBUG
enabled = true
"#;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run_python_output(python: &str, script: &str) -> String {
    zed::process::Command::new(python)
        .args(["-c", script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn write_file(python: &str, path: &str, content: &str) {
    let _ = zed::process::Command::new(python)
        .args([
            "-c",
            "import sys; open(sys.argv[1],'w').write(sys.argv[2])",
            path,
            content,
        ])
        .output();
}


fn detect_venv(root: &str) -> Option<(String, String)> {
    for dir in &[".venv", "venv"] {
        let path = format!("{}/{}/bin/python", root, dir);
        if std::path::Path::new(&path).exists() {
            return Some((path, dir.to_string()));
        }
    }
    None
}

// ── Step 1: Install plugin into .venv/site-packages ──────────────────────────

fn install_plugin(venv_python: &str) {
    // Skip if already installed
    let already = zed::process::Command::new(venv_python)
        .args(["-c", "import pytest_local_plugin; print('ok')"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok")
        .unwrap_or(false);
    if already {
        return;
    }

    // Get site-packages path
    let site = run_python_output(
        venv_python,
        "import site; print(site.getsitepackages()[0])",
    );
    if site.is_empty() {
        return;
    }

    // Write plugin module
    write_file(venv_python, &format!("{}/pytest_local_plugin.py", site), PLUGIN_CODE);

    // Write dist-info so pytest discovers the plugin via pytest11 entry-point
    let di = format!("{}/pytest_local_ini-0.1.0.dist-info", site);
    let _ = zed::process::Command::new("mkdir").args(["-p", &di]).output();
    write_file(venv_python, &format!("{}/entry_points.txt", di),
        "[pytest11]\npytest-local = pytest_local_plugin\n");
    write_file(venv_python, &format!("{}/METADATA", di),
        "Metadata-Version: 2.1\nName: pytest-local-ini\nVersion: 0.1.0\n");
    write_file(venv_python, &format!("{}/RECORD", di), "");
    write_file(venv_python, &format!("{}/INSTALLER", di), "zed-pytest-local\n");
}

// ── Step 2: Create pytest_local.ini ──────────────────────────────────────────

fn create_ini(venv_python: &str, root: &str, worktree: &Worktree) {
    let ini_path = format!("{}/pytest_local.ini", root);
    if std::path::Path::new(&ini_path).exists() {
        return; // already exists
    }

    // Use pytest.ini as template if present, else built-in template
    let content = worktree
        .read_text_file("pytest.ini")
        .map(|existing| {
            format!(
                "# pytest_local.ini — local overrides (gitignored, per-developer)\n\
                 # Based on your pytest.ini — edit freely, never commit.\n\n{}",
                existing
            )
        })
        .unwrap_or_else(|_| DEFAULT_INI_TEMPLATE.to_string());

    write_file(venv_python, &ini_path, &content);
}

// ── Step 3: Update .gitignore ─────────────────────────────────────────────────

fn update_gitignore(venv_python: &str, root: &str, worktree: &Worktree) {
    let gi_path = format!("{}/.gitignore", root);
    let existing = worktree.read_text_file(".gitignore").unwrap_or_default();
    if existing.lines().any(|l| l.trim() == "pytest_local.ini") {
        return;
    }
    let new_content = if existing.is_empty() {
        "pytest_local.ini\n".to_string()
    } else {
        format!("{}\npytest_local.ini\n", existing.trim_end())
    };
    write_file(venv_python, &gi_path, &new_content);
}

// ── Step 4: Write .zed/settings.json interpreter paths ───────────────────────

fn update_zed_settings(venv_python: &str, root: &str, venv_dir: &str) {
    // Use Python for JSON merge to handle existing settings safely
    let script = format!(
        r#"
import json, re, sys
from pathlib import Path

root = Path(sys.argv[1])
venv_python = sys.argv[2]
venv_dir = sys.argv[3]

settings_path = root / '.zed' / 'settings.json'
try:
    raw = settings_path.read_text()
    # strip // comments
    clean = re.sub(r'//[^\n]*', '', raw)
    d = json.loads(clean)
except:
    d = {{}}

changed = False

# pytest_local block
pl = d.setdefault('pytest_local', {{}})
if 'ini_filename' not in pl:
    pl['ini_filename'] = 'pytest_local.ini'
    changed = True
if 'venv_dirs' not in pl:
    pl['venv_dirs'] = ['.venv', 'venv']
    changed = True

# LSP interpreter paths — only fill missing keys
for lsp in ('pyright', 'basedpyright'):
    py_settings = (d.setdefault('lsp', {{}})
                    .setdefault(lsp, {{}})
                    .setdefault('settings', {{}})
                    .setdefault('python', {{}}))
    if 'pythonPath' not in py_settings:
        py_settings['pythonPath'] = venv_python
        py_settings.setdefault('venvPath', '.')
        py_settings.setdefault('venv', venv_dir)
        changed = True

# Terminal PATH — gutter play + integrated terminal use venv Python
venv_bin = str(Path(venv_python).parent)
term = d.setdefault('terminal', {{}})
env_vars = term.setdefault('env', {{}})
existing_path = env_vars.get('PATH', '')
if venv_bin not in existing_path:
    env_vars['PATH'] = venv_bin + (':' + existing_path if existing_path else '')
    changed = True

if changed:
    settings_path.parent.mkdir(exist_ok=True)
    settings_path.write_text(json.dumps(d, indent=2))
    print('updated')
else:
    print('skipped')
"#
    );

    let _ = zed::process::Command::new(venv_python)
        .args(["-c", &script, root, venv_python, venv_dir])
        .output();
}

// ── Extension ─────────────────────────────────────────────────────────────────

struct PytestLocalExtension;

impl zed::Extension for PytestLocalExtension {
    fn new() -> Self {
        PytestLocalExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let root = worktree.root_path();

        // Only activate if a venv is present
        if let Some((venv_python, venv_dir)) = detect_venv(&root) {
            install_plugin(&venv_python);
            create_ini(&venv_python, &root, worktree);
            update_gitignore(&venv_python, &root, worktree);
            update_zed_settings(&venv_python, &root, &venv_dir);
        }

        // Return no-op LSP — satisfies Zed's language server protocol requirement
        // without providing any competing features (pyright/ty remain primary)
        Ok(Command {
            command: "python3".into(),
            args: vec!["-c".into(), NOOP_LSP.into()],
            env: vec![],
        })
    }
}

zed::register_extension!(PytestLocalExtension);
