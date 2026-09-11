# msgint-cli PR #2 public flags certification

This fixture independently certifies the secret-free public command surface proposed by `messaging-intel/msgint-cli#2`.

Exact product inputs copied for this certification:

- `.cli-flags.toml` blob: `23ee5070515f6e307c59115c3dfcfa8904abbb98`
- `src/cli.rs` blob reviewed for package-owned policy resolution: `25f5872452e9a9341820df61e9d4b385d92ff2d3`
- flags2env Rust binding: `377bff1a4e7424eb98377997a232ddc0fc700f59`

The executable harness verifies the copied `.cli-flags.toml` with the official pinned Rust binding. It proves `check-config` and `identity` are accepted as commands while credential-shaped and undeclared options fail closed. The product's private Shared Auth dependency is intentionally outside this test: production compilation remains a separate merge gate.
