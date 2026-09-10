from __future__ import annotations
import re
import tomllib
from pathlib import Path
from typing import Any, Iterable
import ores_mw_config_core as _core
ManifestError = _core.ManifestError
load_manifest = _core.load_manifest
resolve_target = _core.resolve_target
MAX_ENV_BINDINGS = 128
MAX_ENV_DEFAULT_BYTES = 8192
MAX_ENV_DESCRIPTION_BYTES = 512
MAX_FLAGS_CONFIG_BYTES = 256 * 1024
ENV_BINDING_NAME = re.compile('^[a-z][a-z0-9_]{0,63}$')
ENVIRONMENT_KEY = re.compile('^[A-Z_][A-Z0-9_]{0,127}$')
ENV_KINDS = {'string', 'bool', 'integer', 'double', 'json', 'url'}
FLAGS2ENV_KEYS = {'contract', 'require_audit', 'precedence'}
ENV_KEYS = {'name', 'key', 'kind', 'required', 'secret', 'default', 'description'}
_CORE_NORMALIZE = _core.normalize_manifest
_CORE_CHECK = _core.check_referenced_files
_CORE_COMPILE = _core.compile_manifest

def _need(ok: bool, code: str, field: str='') -> None:
    if not ok:
        raise ManifestError(code, field)

def _normalize_flags2env(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    _need(isinstance(value, dict), 'flags2env-object-required', 'flags2env')
    unknown = sorted(set(value) - FLAGS2ENV_KEYS)
    _need(not unknown, 'unknown-flags2env-key', f'flags2env.{unknown[0]}' if unknown else 'flags2env')
    contract = value.get('contract')
    _need(isinstance(contract, str) and contract == '.cli-flags.toml', 'canonical-flags-contract-required', 'flags2env.contract')
    require_audit = value.get('require_audit')
    _need(type(require_audit) is bool and require_audit, 'flags2env-audit-required', 'flags2env.require_audit')
    precedence = value.get('precedence')
    _need(precedence == 'argv-over-env', 'flags2env-precedence-required', 'flags2env.precedence')
    return {'contract': contract, 'requireAudit': True, 'precedence': precedence}

def _normalize_env(value: Any, flags2env: dict[str, Any] | None) -> list[dict[str, Any]] | None:
    if value is None:
        return None
    _need(flags2env is not None, 'env-requires-flags2env', 'env')
    _need(isinstance(value, list) and 0 < len(value) <= MAX_ENV_BINDINGS, 'env-binding-count', 'env')
    names: set[str] = set()
    keys: set[str] = set()
    normalized: list[dict[str, Any]] = []
    for index, item in enumerate(value):
        prefix = f'env[{index}]'
        _need(isinstance(item, dict), 'env-binding-object-required', prefix)
        unknown = sorted(set(item) - ENV_KEYS)
        _need(not unknown, 'unknown-env-binding-key', f'{prefix}.{unknown[0]}' if unknown else prefix)
        name = item.get('name')
        _need(isinstance(name, str) and ENV_BINDING_NAME.fullmatch(name) is not None, 'invalid-env-binding-name', f'{prefix}.name')
        _need(name not in names, 'duplicate-env-binding-name', f'{prefix}.name')
        names.add(name)
        key = item.get('key')
        _need(isinstance(key, str) and ENVIRONMENT_KEY.fullmatch(key) is not None, 'invalid-environment-key', f'{prefix}.key')
        _need(key not in keys, 'duplicate-environment-key', f'{prefix}.key')
        keys.add(key)
        kind = item.get('kind')
        _need(kind in ENV_KINDS, 'invalid-env-kind', f'{prefix}.kind')
        required = item.get('required')
        secret = item.get('secret')
        _need(type(required) is bool, 'boolean-required', f'{prefix}.required')
        _need(type(secret) is bool, 'boolean-required', f'{prefix}.secret')
        default = item.get('default')
        if default is not None:
            _need(isinstance(default, str), 'string-required', f'{prefix}.default')
            _need(len(default.encode()) <= MAX_ENV_DEFAULT_BYTES, 'env-default-too-large', f'{prefix}.default')
            _need(not secret, 'secret-default-forbidden', f'{prefix}.default')
        description = item.get('description')
        if description is not None:
            _need(isinstance(description, str), 'string-required', f'{prefix}.description')
            _need(len(description.encode()) <= MAX_ENV_DESCRIPTION_BYTES, 'env-description-too-large', f'{prefix}.description')
        row: dict[str, Any] = {'name': name, 'key': key, 'kind': kind, 'required': required, 'secret': secret}
        if default is not None:
            row['defaultValue'] = default
        if description is not None:
            row['description'] = description
        normalized.append(row)
    return normalized

def normalize_manifest(raw: dict[str, Any]) -> dict[str, Any]:
    stripped = dict(raw)
    flags2env = _normalize_flags2env(stripped.pop('flags2env', None))
    env = _normalize_env(stripped.pop('env', None), flags2env)
    result = _CORE_NORMALIZE(stripped)
    if flags2env is not None:
        result['flags2env'] = flags2env
    if env is not None:
        result['env'] = env
    return result

def _collect_flag_env_keys(value: Any) -> set[str]:
    result: set[str] = set()
    if isinstance(value, dict):
        candidate = value.get('env')
        if isinstance(candidate, str):
            result.add(candidate)
        for nested in value.values():
            result.update(_collect_flag_env_keys(nested))
    elif isinstance(value, list):
        for nested in value:
            result.update(_collect_flag_env_keys(nested))
    return result

def _load_flags_contract(manifest: dict[str, Any], repo_root: Path) -> tuple[bytes, dict[str, Any]] | None:
    flags2env = manifest.get('flags2env')
    if flags2env is None:
        return None
    path = _core.checked_repo_path(repo_root, flags2env['contract'], 'file')
    data = _core.read_bounded(path, MAX_FLAGS_CONFIG_BYTES)
    try:
        config = tomllib.loads(data.decode())
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise ManifestError('invalid-flags2env-toml', flags2env['contract']) from exc
    _need(isinstance(config, dict), 'flags2env-object-required', flags2env['contract'])
    return (data, config)

def check_referenced_files(manifest: dict[str, Any], repo_root: Path) -> None:
    flags = _load_flags_contract(manifest, repo_root)
    if flags is not None:
        _, config = flags
        exposed = _collect_flag_env_keys(config)
        for binding in manifest.get('env', []):
            if binding['secret']:
                _need(binding['key'] not in exposed, 'secret-env-exposed-as-cli-flag', binding['name'])
    _CORE_CHECK(manifest, repo_root)

def _install_core_hooks() -> None:
    _core.normalize_manifest = normalize_manifest
    _core.check_referenced_files = check_referenced_files

def compile_manifest(manifest_path: Path, repo_root: Path, out_dir: Path) -> dict[str, Any]:
    _install_core_hooks()
    return _CORE_COMPILE(manifest_path, repo_root, out_dir)

def main(argv: Iterable[str] | None=None) -> int:
    _install_core_hooks()
    return _core.main(argv)
if __name__ == '__main__':
    raise SystemExit(main())
