#!/bin/sh
# Makes what the Mac App Store copy is signed with, once, through the App Store
# Connect API (docs/mac-app-store-owner-steps.md, step 2). Nothing is changed in
# the Apple account unless --apply is given; without it the script only says what
# exists and what it would make.
#
#   scripts/mas-signing-setup.sh            look only
#   scripts/mas-signing-setup.sh --apply    make what is missing
#
# What it makes (team 6Z95MR26LZ):
#   1. "Apple Distribution: Reza Pirighadim (6Z95MR26LZ)"          signs Lexpad.app
#   2. "3rd Party Mac Developer Installer: Reza Pirighadim (...)"  signs the .pkg
#      Each private key is made on this Mac and imported, not extractable, into
#      ~/Library/Keychains/lexpad-signing.keychain-db (the dmg's keychain); only
#      the certificate request goes to Apple.
#   3. The "Mac App Store" provisioning profile for app.lexpad.ios, saved to
#      ~/.appstoreconnect/profiles/Lexpad_Mac_App_Store.provisionprofile.
#
# It uses the App Store Connect API key in ~/.appstoreconnect/lexpad.env (never
# printed). Nothing it makes is ever committed: this repository is public.
set -eu

apply=0
[ "${1:-}" = "--apply" ] && apply=1

team=6Z95MR26LZ
kc="$HOME/Library/Keychains/lexpad-signing.keychain-db"
service="Lexpad Local Signing keychain"
profiles="$HOME/.appstoreconnect/profiles"

die() {
  echo "mas-signing-setup: $*" >&2
  exit 1
}

[ -f "$kc" ] || die "$kc is missing (scripts/make-developer-id-csr.sh makes it)"
kpw=$(security find-generic-password -a "$USER" -s "$service" -w 2>/dev/null) || die "the keychain's password is not in the login keychain"
security unlock-keychain -p "$kpw" "$kc"

if [ -z "${ASC_KEY_ID:-}" ] && [ -f "$HOME/.appstoreconnect/lexpad.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$HOME/.appstoreconnect/lexpad.env"
  set +a
fi
: "${ASC_KEY_ID:?set ASC_KEY_ID (or ~/.appstoreconnect/lexpad.env)}"
: "${ASC_ISSUER_ID:?set ASC_ISSUER_ID}"
ASC_KEY_PATH=${ASC_KEY_PATH:-$HOME/.appstoreconnect/private_keys/AuthKey_${ASC_KEY_ID}.p8}
export ASC_KEY_ID ASC_ISSUER_ID ASC_KEY_PATH

work=$(mktemp -d)
chmod 700 "$work"
trap 'rm -P "$work"/* 2>/dev/null; rm -rf "$work"' EXIT

# make_csr <name>: a new key in the keychain and its request in $work/<name>.csr
make_csr() {
  openssl genrsa -out "$work/$1.pem" 2048 2>/dev/null
  if head -1 "$work/$1.pem" | grep -q 'BEGIN PRIVATE KEY'; then
    openssl pkey -in "$work/$1.pem" -traditional -out "$work/$1.k" && mv "$work/$1.k" "$work/$1.pem"
  fi
  openssl req -new -key "$work/$1.pem" -out "$work/$1.csr" -subj "/CN=Reza Pirighadim" 2>/dev/null
  security import "$work/$1.pem" -k "$kc" -t priv -f openssl -x \
    -T /usr/bin/codesign -T /usr/bin/productbuild -T /usr/bin/security
  rm -P "$work/$1.pem"
}

has_identity() {
  security find-identity -v "$kc" | grep -q "\"$1\""
}

need=""
has_identity "Apple Distribution: Reza Pirighadim ($team)" || need="$need DISTRIBUTION"
has_identity "3rd Party Mac Developer Installer: Reza Pirighadim ($team)" || need="$need MAC_INSTALLER_DISTRIBUTION"
[ -f "$profiles/Lexpad_Mac_App_Store.provisionprofile" ] || need="$need PROFILE"

if [ -z "$need" ]; then
  echo "mas-signing-setup: everything is in place"
  exit 0
fi
echo "mas-signing-setup: missing:$need"
if [ "$apply" = 0 ]; then
  echo "mas-signing-setup: run again with --apply to make it (after the owner said yes)"
  exit 0
fi

for kind in $need; do
  case "$kind" in DISTRIBUTION | MAC_INSTALLER_DISTRIBUTION) make_csr "$kind" ;; esac
done
mkdir -p "$profiles"
chmod 700 "$profiles"

WORK="$work" NEED="$need" TEAM="$team" PROFILES="$profiles" python3 -I - <<'PY'
import base64, json, os, pathlib, ssl, time, urllib.request
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature

API = "https://api.appstoreconnect.apple.com"
work, need = pathlib.Path(os.environ["WORK"]), os.environ["NEED"].split()
key = serialization.load_pem_private_key(pathlib.Path(os.environ["ASC_KEY_PATH"]).read_bytes(), password=None)

def b64(raw): return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()
def token():
    now = int(time.time())
    head = b64(json.dumps({"alg": "ES256", "kid": os.environ["ASC_KEY_ID"], "typ": "JWT"}).encode())
    body = b64(json.dumps({"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 900, "aud": "appstoreconnect-v1"}).encode())
    r, s = decode_dss_signature(key.sign(f"{head}.{body}".encode(), ec.ECDSA(hashes.SHA256())))
    return f"{head}.{body}." + b64(r.to_bytes(32, "big") + s.to_bytes(32, "big"))
def ctx():
    try:
        import certifi
        return ssl.create_default_context(cafile=certifi.where())
    except ImportError:
        return ssl.create_default_context(cafile="/etc/ssl/cert.pem")
def call(method, path, body=None):
    req = urllib.request.Request(API + path, method=method, data=json.dumps(body).encode() if body else None)
    req.add_header("Authorization", f"Bearer {token()}")
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60, context=ctx()) as res:
        return json.loads(res.read() or b"{}")

certs = {}
for kind in need:
    if kind == "PROFILE":
        continue
    csr = (work / f"{kind}.csr").read_text()
    made = call("POST", "/v1/certificates", {"data": {"type": "certificates", "attributes": {"certificateType": kind, "csrContent": csr}}})
    content = made["data"]["attributes"]["certificateContent"]
    (work / f"{kind}.cer").write_bytes(base64.b64decode(content))
    certs[kind] = made["data"]["id"]
    print(f"mas-signing-setup: made certificate {kind}")

if "PROFILE" in need:
    bundle = call("GET", "/v1/bundleIds?filter[identifier]=app.lexpad.ios&limit=5")["data"]
    bundle = [b for b in bundle if b["attributes"]["identifier"] == "app.lexpad.ios"][0]
    dist = certs.get("DISTRIBUTION")
    if dist is None:
        listed = call("GET", "/v1/certificates?filter[certificateType]=DISTRIBUTION&limit=50")["data"]
        dist = sorted(listed, key=lambda c: c["attributes"]["expirationDate"])[-1]["id"]
    made = call("POST", "/v1/profiles", {"data": {"type": "profiles",
        "attributes": {"name": "Lexpad Mac App Store", "profileType": "MAC_APP_STORE"},
        "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": bundle["id"]}},
                          "certificates": {"data": [{"type": "certificates", "id": dist}]}}}})
    out = pathlib.Path(os.environ["PROFILES"]) / "Lexpad_Mac_App_Store.provisionprofile"
    out.write_bytes(base64.b64decode(made["data"]["attributes"]["profileContent"]))
    out.chmod(0o600)
    print(f"mas-signing-setup: saved the profile to {out}")
PY

for kind in DISTRIBUTION MAC_INSTALLER_DISTRIBUTION; do
  if [ -f "$work/$kind.cer" ]; then
    security import "$work/$kind.cer" -k "$kc" -T /usr/bin/codesign -T /usr/bin/productbuild
  fi
done
security set-key-partition-list -S apple-tool:,apple:,codesign:,productbuild: -s -k "$kpw" "$kc" >/dev/null
unset kpw
security find-identity -v "$kc" | sed 's/[0-9A-F]\{40\}/<hash>/'
echo "mas-signing-setup: done; scripts/release-mas.sh can sign and package now"
