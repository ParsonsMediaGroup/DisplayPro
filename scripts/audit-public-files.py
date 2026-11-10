"""Audit tracked files without printing secret values. Run before a public push."""
import json
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
tracked = subprocess.check_output(['git', 'ls-files', '-z'], cwd=root).decode().split('\0')
patterns = {
    'private key': rb'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----',
    'GitHub credential': rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b',
    'AWS access key': rb'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b',
    'OpenAI-style secret': rb'\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{30,}\b',
    'credential-bearing URL': rb'https?://[^\s/:]{2,}:[^\s/@]{6,}@',
    'literal secret assignment': rb'(?i)(?:api[_-]?key|client[_-]?secret|access[_-]?token|password)\s*[=:]\s*[\"\x27][A-Za-z0-9_+/.=-]{20,}[\"\x27]',
}
private_dirs = {'verification', 'settings-imports', 'settings-exports', 'settings-backup', 'instance', 'user-data', 'node_modules', 'target', '.idea', '.vscode'}
failures = []
for name in filter(None, tracked):
    path = Path(name)
    if any(part in private_dirs for part in path.parts) or (path.name.startswith('.env') and path.name != '.env.example') or '.displayprolite-settings.' in name or path.suffix.lower() in {'.pem', '.key', '.p12', '.pfx', '.exe', '.scr', '.msi', '.dmp'}:
        failures.append((name, 'private/generated filename'))
    # Audit the actual index content that would be committed, not a possibly
    # different unstaged working copy.
    data = subprocess.check_output(['git', 'show', ':' + name], cwd=root)
    for description, pattern in patterns.items():
        if re.search(pattern, data):
            failures.append((name, description))
    if path.suffix == '.json':
        try:
            value = json.loads(data)
            if isinstance(value, dict) and value.get('format') == 'DisplayProLite.settings':
                failures.append((name, 'private settings export (any filename)'))
        except (ValueError, UnicodeError):
            pass
if failures:
    for name, reason in failures:
        print(f'BLOCK: {name}: {reason}')
    raise SystemExit(1)
print(f'PASS: audited {len(list(filter(None, tracked)))} tracked files; no configured secret/private-file patterns found.')
