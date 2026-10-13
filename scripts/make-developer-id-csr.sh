#!/bin/sh
# Makes the key and certificate signing request for Lexpad's "Developer ID
# Application" certificate, and later imports the certificate Apple issues.
#
#   scripts/make-developer-id-csr.sh            # key + CSR
#   scripts/make-developer-id-csr.sh --import ~/Downloads/developerID_application.cer
#
# The private key is made here and goes straight into the dedicated signing
# keychain ~/Library/Keychains/lexpad-signing.keychain-db, as a
# non-extractable key that only codesign may use; the plain key file is
# erased at once. That is the keychain the "Lexpad Local Signing" certificate
# already lives in: its random password is the login keychain's generic
# password "Lexpad Local Signing keychain", the same one release-mac.sh uses.
# When the keychain does not exist yet it is made the same way (README,
# "macOS signing").
#
# Then, by hand (only the Account Holder may make a Developer ID certificate):
# developer.apple.com → Certificates → + → Developer ID Application →
# profile type G2 Sub-CA → upload the .certSigningRequest → download the .cer
# → run this script with --import <the .cer>. The certificate then pairs with
# the key in the keychain and release-mac.sh picks
# "Developer ID Application: Reza Pirighadim (6Z95MR26LZ)" by itself.
#
# Nothing here is a secret that may be committed: the CSR is public, the key
# never leaves the keychain. This repository is public; keep it that way.
set -eu

team=6Z95MR26LZ
developer_id="Developer ID Application: Reza Pirighadim ($team)"
kc="$HOME/Library/Keychains/lexpad-signing.keychain-db"
service="Lexpad Local Signing keychain"
out_dir="${LEXPAD_CSR_DIR:-$HOME/.appstoreconnect}"
csr="$out_dir/DeveloperIDApplication-$team.certSigningRequest"

die() {
  echo "make-developer-id-csr: $*" >&2
  exit 1
}

# The keychain and its password, made once if they are not there yet.
if ! kpw=$(security find-generic-password -a "$USER" -s "$service" -w 2>/dev/null); then
  [ -f "$kc" ] && die "$kc exists but its password is not in the login keychain as \"$service\""
  kpw=$(openssl rand -hex 24)
  security add-generic-password -a "$USER" -s "$service" -w "$kpw" "$HOME/Library/Keychains/login.keychain-db"
fi
if [ ! -f "$kc" ]; then
  security create-keychain -p "$kpw" "$kc"
  security set-keychain-settings "$kc"
fi
security unlock-keychain -p "$kpw" "$kc"
case "$(security list-keychains -d user)" in
  *lexpad-signing.keychain-db*) ;;
  *)
    # Keep what is on the list and add this keychain to it.
    # shellcheck disable=SC2046
    security list-keychains -d user -s $(security list-keychains -d user | tr -d '"') "$kc"
    ;;
esac

if [ "${1:-}" = --import ]; then
  cer=${2:-}
  [ -f "$cer" ] || die "usage: $0 --import <developerID_application.cer>"
  subject=$(openssl x509 -inform DER -in "$cer" -noout -subject 2>/dev/null ||
    openssl x509 -in "$cer" -noout -subject)
  case "$subject" in
    *"$developer_id"*) ;;
    *) die "this certificate is not \"$developer_id\": $subject" ;;
  esac
  security import "$cer" -k "$kc" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kpw" "$kc" >/dev/null
  unset kpw
  # A valid identity needs Apple's Developer ID G2 intermediate, which Xcode
  # installs; without it find-identity -v leaves the identity out.
  if security find-identity -v -p codesigning "$kc" | grep -q "\"$developer_id\""; then
    echo "make-developer-id-csr: \"$developer_id\" is ready in $kc"
    echo "make-developer-id-csr: scripts/release-mac.sh now signs with it and notarizes"
  else
    security find-identity -p codesigning "$kc" >&2
    die "the certificate is in the keychain but not a valid identity (missing key or Developer ID G2 intermediate?)"
  fi
  exit 0
fi
[ $# -eq 0 ] || die "usage: $0 [--import <developerID_application.cer>]"

if security find-identity -v -p codesigning "$kc" | grep -q "\"$developer_id\""; then
  die "\"$developer_id\" is already in $kc; nothing to do"
fi
[ -f "$csr" ] && die "$csr already exists (a key for it is in the keychain); upload that one, or delete both first"

email=${LEXPAD_CSR_EMAIL:-$(git -C "$(dirname "$0")" config user.email || true)}
[ -n "$email" ] || die "set LEXPAD_CSR_EMAIL to the Apple ID e-mail of the account holder"

mkdir -p "$out_dir"
chmod 700 "$out_dir"
work=$(mktemp -d)
trap 'rm -P "$work"/* 2>/dev/null; rmdir "$work" 2>/dev/null' EXIT
openssl genrsa -out "$work/key.pem" 2048 2>/dev/null
# security import -f openssl wants PKCS#1 ("BEGIN RSA PRIVATE KEY"); OpenSSL 3
# writes PKCS#8, LibreSSL (/usr/bin/openssl) already writes PKCS#1.
if head -1 "$work/key.pem" | grep -q 'BEGIN PRIVATE KEY'; then
  openssl pkey -in "$work/key.pem" -traditional -out "$work/key1.pem"
  mv "$work/key1.pem" "$work/key.pem"
fi
openssl req -new -key "$work/key.pem" -out "$csr" \
  -subj "/emailAddress=$email/CN=Reza Pirighadim" 2>/dev/null
# Non-extractable (-x), usable by codesign without a prompt; the plain file
# is overwritten and removed by the trap.
security import "$work/key.pem" -k "$kc" -t priv -f openssl -x -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kpw" "$kc" >/dev/null
unset kpw

echo "make-developer-id-csr: private key in $kc (non-extractable, codesign only)"
echo "make-developer-id-csr: CSR at $csr"
echo "Next: developer.apple.com → Certificates → + → Developer ID Application (G2 Sub-CA) → upload the CSR,"
echo "download the .cer, then: scripts/make-developer-id-csr.sh --import <the .cer>"
