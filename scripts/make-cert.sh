#!/bin/sh
# Creates the self-signed "TypeThru Release" code signing certificate in the login keychain,
# once. Releases signed with it keep the Accessibility permission across in-app updates.
# The certificate holds only that name: no Apple ID, email, or team. macOS asks for your
# password to trust it for code signing.
set -eu
NAME='TypeThru Release'
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "\"$NAME\" already exists in your login keychain. Keep using it, so updates keep Accessibility."
    exit 0
fi
WORK=$(mktemp -d "${TMPDIR:-/tmp}/typethru-cert.XXXXXX")
cleanup() {
    rm -f "$WORK/openssl.cnf" "$WORK/key.pem" "$WORK/cert.pem" "$WORK/cert.p12"
    rmdir "$WORK"
}
trap cleanup EXIT
cat > "$WORK/openssl.cnf" <<EOF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF
# DECISION: macOS's own LibreSSL, whose PKCS#12 files the keychain imports (OpenSSL 3's need -legacy).
# DECISION: valid for 20 years; a new certificate would cost every user Accessibility once.
/usr/bin/openssl req -x509 -newkey rsa:3072 -nodes -days 7300 -config "$WORK/openssl.cnf" \
    -keyout "$WORK/key.pem" -out "$WORK/cert.pem" 2>/dev/null
PASS=$(/usr/bin/openssl rand -hex 16)
/usr/bin/openssl pkcs12 -export -inkey "$WORK/key.pem" -in "$WORK/cert.pem" -name "$NAME" \
    -out "$WORK/cert.p12" -passout "pass:$PASS"
security import "$WORK/cert.p12" -k "$KEYCHAIN" -P "$PASS" -T /usr/bin/codesign >/dev/null
echo 'macOS will ask for your password to trust the certificate for code signing.'
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$WORK/cert.pem"
echo "Created \"$NAME\". The first build may ask to let codesign use it: choose Always Allow."
echo 'Back it up: Keychain Access > My Certificates > TypeThru Release > Export (.p12).'
