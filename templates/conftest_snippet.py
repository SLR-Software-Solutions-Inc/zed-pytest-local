# BEGIN pytest-local-ini (managed by zed-pytest-local extension — do not remove this block)
import configparser
import json
from pathlib import Path


def _apply_ini_override(config, key, value):
    """Apply a single ini override — compatible across pytest 6, 7, 8, 9+."""
    # Method 1: pytest 9+ — _inicfg is a dict[str, ConfigValue]
    try:
        from _pytest.config import ConfigValue  # type: ignore[attr-defined]
        if hasattr(config, '_inicfg') and isinstance(config._inicfg, dict):
            config._inicfg[key] = ConfigValue(value=value, origin="override", mode="ini")
            if hasattr(config, '_inicache'):
                config._inicache.pop(key, None)
            return
    except (ImportError, Exception):
        pass

    # Method 2: pytest 6/7/8 public API
    if hasattr(config, 'override_ini'):
        try:
            config.override_ini(key, value)
            return
        except Exception:
            pass

    # Method 3: last resort — direct cache write (may have type issues)
    try:
        if hasattr(config, '_inicache'):
            config._inicache[key] = value
    except Exception:
        pass


def pytest_configure(config):
    """Load pytest_local.ini (or configured name) and override active pytest settings.

    Reads ini_filename from .zed/settings.json if present, falls back to
    pytest_local.ini. The ini file is intentionally gitignored — allows
    per-developer local overrides without touching the committed pytest.ini.

    Precedence:  CLI args  >  pytest_local.ini  >  pytest.ini

    Sections:
        [pytest]       — standard pytest ini options (override pytest.ini)
        [pytest_local] — convenience options for this plugin:
                           enabled = true  ->  enables log_cli + DEBUG level
    """
    try:
        rootdir = Path(str(config.rootdir))
    except AttributeError:
        return

    # Read ini_filename from .zed/settings.json (project-local Zed config)
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
        return

    parser = configparser.ConfigParser()
    parser.read(local_ini)

    # [pytest] section: standard pytest ini overrides
    if parser.has_section("pytest"):
        for key, value in parser["pytest"].items():
            _apply_ini_override(config, key, value)

    # [pytest_local] section: convenience toggles
    if parser.has_section("pytest_local"):
        enabled = parser.get("pytest_local", "enabled", fallback="").strip().lower()
        if enabled == "true":
            _apply_ini_override(config, "log_cli", "true")
            _apply_ini_override(config, "log_cli_level", "DEBUG")
# END pytest-local-ini
