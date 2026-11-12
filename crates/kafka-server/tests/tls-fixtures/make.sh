#!/usr/bin/env bash
# Remakes the test certificates in this directory (README.md says what each is). Needs openssl; LibreSSL, which macOS
# ships, works. Run it from anywhere: `crates/kafka-server/tests/tls-fixtures/make.sh`. They expire in about a hundred
# years (36500 days), so nothing here needs renewing.
set -euo pipefail
# -sha256 everywhere: LibreSSL signs with SHA-1 unless told otherwise, and rustls refuses SHA-1 signatures.
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"
days=36500

ca() { # name subject -> name.key, name.pem (a self-signed CA)
  openssl ecparam -name prime256v1 -genkey -noout -out "$1.key"
  openssl req -x509 -new -sha256 -key "$1.key" -out "$1.pem" -days $days -subj "$2" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
}

ca root "/CN=doze test root"
ca other "/CN=doze other root"

# An intermediate signed by the root, so the chain the server sends has two certificates.
openssl ecparam -name prime256v1 -genkey -noout -out mid.key
openssl req -new -key mid.key -out mid.csr -subj "/CN=doze test intermediate"
printf 'basicConstraints=critical,CA:TRUE,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\n' >mid.ext
openssl x509 -req -in mid.csr -CA root.pem -CAkey root.key -CAcreateserial -sha256 -out mid.pem -days $days -extfile mid.ext

leaf() { # name key-file -> name.pem (a leaf for localhost and 127.0.0.1, signed by the intermediate)
  openssl req -new -key "$2" -out "$1.csr" -subj "/CN=localhost"
  printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n' >"$1.ext"
  openssl x509 -req -in "$1.csr" -CA mid.pem -CAkey mid.key -CAcreateserial -sha256 -out "$1.pem" -days $days -extfile "$1.ext"
}

# An elliptic-curve leaf, its key as SEC1 ("EC PRIVATE KEY") and as PKCS#8.
openssl ecparam -name prime256v1 -genkey -noout -out ec-key-sec1.pem
openssl pkcs8 -topk8 -nocrypt -in ec-key-sec1.pem -out ec-key-pkcs8.pem
leaf ec ec-key-sec1.pem
# An RSA leaf, its key as PKCS#1 ("RSA PRIVATE KEY") and as PKCS#8.
openssl genrsa -out rsa-key-pkcs1.pem 2048
openssl pkcs8 -topk8 -nocrypt -in rsa-key-pkcs1.pem -out rsa-key-pkcs8.pem
leaf rsa rsa-key-pkcs1.pem

cp root.pem "$here/root.pem"
cp other.pem "$here/other-root.pem"
cat ec.pem mid.pem >"$here/ec-chain.pem"
cat rsa.pem mid.pem >"$here/rsa-chain.pem"
cp ec.pem "$here/ec-leaf-only.pem"
cp ec-key-sec1.pem ec-key-pkcs8.pem rsa-key-pkcs1.pem rsa-key-pkcs8.pem "$here/"
