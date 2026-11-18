"""Check the Git index before publication. Reports paths/rules, never secret values."""
from pathlib import Path
import re
import struct
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
def indexed_blob(name):
    return subprocess.check_output(["git", "show", f":{name}"], cwd=root)

result = subprocess.run(["git", "ls-files", "-z"], cwd=root, capture_output=True, check=True)
paths = [p.decode("utf-8") for p in result.stdout.split(b"\0") if p]
if not paths:
    raise SystemExit("No indexed files; stage the intended publication before auditing.")
issues = []
for name in paths:
    path = Path(name)
    if (path.suffix.lower() in {".exe", ".dll", ".onnx", ".safetensors", ".wav", ".mp3", ".pyc", ".log", ".npz"}
            or "__pycache__" in path.parts or path.name in {"runtime.json", "settings.json", ".env"}):
        issues.append((name, "private/runtime/generated artifact"))
    data = indexed_blob(name)
    if len(data) > 5_000_000:
        issues.append((name, "unexpected large file"))
    if b"\0" in data[:8192]:
        continue
    text = data.decode("utf-8", errors="replace")
    checks = {
        "provider token": r"\b(?:sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{30,})\b",
        "private key": r"-----BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY-----",
        "machine-specific path": r"(?i)(?:[A-Z]:[/\\](?:codex|users)[/\\])",
        "persisted encrypted key": r'"ai_key_protected"\s*:\s*"[A-Za-z0-9+/=]{40,}"',
    }
    for rule, pattern in checks.items():
        if re.search(pattern, text):
            issues.append((name, rule))

ico = indexed_blob("assets/app.ico")
reserved, kind, count = struct.unpack_from("<HHH", ico)
if (reserved, kind, count) != (0, 1, 9):
    issues.append(("assets/app.ico", "missing multi-resolution Windows icon"))
for index in range(count):
    width, height, _, _, planes, bits, length, offset = struct.unpack_from("<BBBBHHII", ico, 6 + index * 16)
    if planes != 1 or bits != 32 or ico[offset:offset+8] != b"\x89PNG\r\n\x1a\n" or offset + length > len(ico):
        issues.append(("assets/app.ico", "invalid icon frame"))
for size in (32, 128):
    if len(indexed_blob(f"assets/icon-{size}.rgba")) != size * size * 4:
        issues.append((f"assets/icon-{size}.rgba", "invalid RGBA dimensions"))
if issues:
    for name, rule in issues:
        print(f"FAIL {name}: {rule}")
    sys.exit(1)
print(f"PASS: {len(paths)} indexed files; artifact, secret-pattern, local-path and icon checks")
