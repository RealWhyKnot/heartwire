# Security

In HTTP mode the app listens on the port you set (8080 by default) on every
network interface, and accepts a POST holding a number. The other modes don't
open a port. Updates come from this repository's releases and are checked
against the SHA-256 in the release's integrity file before they're installed.

Report a security problem by opening a
[private advisory](https://github.com/RealWhyKnot/hr-osc-rust/security/advisories/new)
rather than a public issue.
