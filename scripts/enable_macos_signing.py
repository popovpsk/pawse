import re
import sys
from pathlib import Path

MANIFEST = Path(__file__).resolve().parent.parent / "crates" / "pawse" / "Cargo.toml"
SECTION = "[package.metadata.packager.macos]"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: enable_macos_signing.py <signing-identity>", file=sys.stderr)
        return 2
    identity = sys.argv[1]

    text = MANIFEST.read_text()
    if SECTION not in text:
        print(f"{SECTION} not found in {MANIFEST}", file=sys.stderr)
        return 1

    line = f'signing-identity = "{identity}"'
    if re.search(r"(?m)^\s*signing-identity\s*=", text):
        text = re.sub(r"(?m)^\s*signing-identity\s*=.*$", line, text, count=1)
    else:
        text = text.replace(SECTION, f"{SECTION}\n{line}", 1)

    MANIFEST.write_text(text)
    print(f"signing-identity set to {identity!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
