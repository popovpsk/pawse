#!/usr/bin/env bash
set -euo pipefail

IDENTITY="Pawse Self Signed"
OUT="${1:-$HOME/.pawse-signing}"
PASSWORD="${2:-}"

if [ -z "$PASSWORD" ]; then
  read -rsp "Choose a password for the .p12 (you will store it as a GitHub secret): " PASSWORD
  echo
fi
[ -n "$PASSWORD" ] || { echo "empty password, aborting"; exit 1; }

mkdir -p "$OUT"
chmod 700 "$OUT"
KEY="$OUT/key.pem"
CERT="$OUT/cert.pem"
P12="$OUT/pawse-signing.p12"
B64="$OUT/pawse-signing.p12.base64"

if [ -f "$P12" ]; then
  echo "Refusing to overwrite existing cert at $P12"
  echo "Reusing the same cert is the whole point — delete it manually only if you know what you are doing."
  exit 1
fi

openssl req -x509 -newkey rsa:2048 -keyout "$KEY" -out "$CERT" -days 3650 -nodes \
  -subj "/CN=$IDENTITY" \
  -addext "basicConstraints=critical,CA:false" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"

openssl pkcs12 -export -inkey "$KEY" -in "$CERT" -out "$P12" -name "$IDENTITY" -passout pass:"$PASSWORD"
base64 -i "$P12" | tr -d '\n' > "$B64"

security import "$P12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$PASSWORD" -T /usr/bin/codesign -A

echo
echo "Created self-signed code-signing identity: \"$IDENTITY\""
echo "  key/cert/p12: $OUT  (keep private, never commit)"
echo "  imported into your login keychain (local signing works now)"
echo
echo "GitHub secrets to set:"
echo "  APPLE_CERTIFICATE           = contents of $B64"
echo "  APPLE_CERTIFICATE_PASSWORD  = the password you just chose"
echo
echo "If macOS prompts for keychain access the first time you sign, click \"Always Allow\"."
