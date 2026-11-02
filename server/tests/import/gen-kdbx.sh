#!/bin/sh
# Generates the KDBX test vaults (VM 116). Password for all of them: "lordos-prueba"
#   kx3-aeskdf.kdbx: KeePassXC 2.7.10 (keepassxc-cli import of sample.xml): KDBX 3.1, AES-256, AES-KDF
#   kx4-*.kdbx: pykeepass 4.2.0 (KDBX 4), with the same entries
set -e
cd "$(dirname "$0")"
rm -f kx*.kdbx kx-*.key kx-*.keyx
printf 'lordos-prueba\nlordos-prueba\n' | keepassxc-cli import -q -p -t 100 sample.xml kx3-aeskdf.kdbx
venv/bin/python3 conv.py
