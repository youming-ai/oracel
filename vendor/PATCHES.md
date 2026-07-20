# Local dependency patches

These crates are temporary source patches for transitive Alloy dependencies. Keep their upstream
versions aligned with `Cargo.lock` and remove the patches once upstream releases eliminate the
RustSec informational warnings.

- `alloy-primitives` 1.6.1: replace unmaintained `paste` with drop-in successor `pastey`.
- `syn-solidity` 1.6.1: replace unmaintained `paste` with drop-in successor `pastey`.
- `ruint` 1.19.0: remove unused optional integrations for ark-ff 0.3, 0.4, and 0.5. Those optional
  dependency graphs retain unmaintained procedural macros even though this project enables none of
  the integrations. ark-ff 0.6 support remains available.

The patched crates retain their original upstream licenses as declared in their `Cargo.toml` files.
