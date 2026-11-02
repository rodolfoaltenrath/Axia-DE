# Security policy

The full policy (scope, safe harbour, response times) is at
**[lordos.org/legal/security](https://lordos.org/legal/security/)**.

## Reporting a vulnerability

Please report vulnerabilities **privately**, never in a public issue:

- Use the IPERFEX contact form, <https://www.iperfex.com/contacto/?lang=en>, and put **"Security"**
  in the subject.
- Once this repository is public, GitHub private vulnerability reporting will be available too.

Include the LordOS version (shown in the About LordOS app), how you installed it (ISO,
Raspberry Pi 5 image or Debian package), the steps to reproduce and the impact. English, Spanish and
Portuguese are all fine.

## Supported versions

Until 1.0, only the **latest beta** gets security fixes. Debian packages get their fixes from
Debian's security updates, installed through the Store or apt. See
[lordos.org/legal/support-lifecycle](https://lordos.org/legal/support-lifecycle/).

## Verifying releases

Release files are listed in `SHA256SUMS`, signed with the LordOS release key (Ed25519,
`tools/release/lordos-release.pub.pem`). Fingerprint (SHA-256 of the DER public key):

```
c7155b8e6bbbb180f907381994d06af76d333016cd6f3facf67cadd053ea108e
```

How to check a download and how `.lpk` apps and Debian packages are signed:
[lordos.org/legal/code-signing](https://lordos.org/legal/code-signing/) and
[docs/SIGNING.md](docs/SIGNING.md).
