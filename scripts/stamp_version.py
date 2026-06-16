import pathlib
import re
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: stamp_version.py <X.Y.Z>")

version = sys.argv[1]
if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+].*)?", version):
    raise SystemExit(f"not a semver version: {version!r}")

path = pathlib.Path("Cargo.toml")
text = path.read_text(encoding="utf-8")
new_text = re.sub(r'(?m)^version = "[^"]*"', f'version = "{version}"', text, count=1)
if new_text == text:
    raise SystemExit("could not find a [workspace.package] version line to stamp")

path.write_text(new_text, encoding="utf-8")
print(f"stamped workspace version -> {version}")
