# msgint-cli PR #2 public flags certification

This fixture independently certifies the secret-free public command surface proposed by `messaging-intel/msgint-cli#2`.

Exact product inputs copied or reviewed for this certification:

- `.cli-flags.toml` blob: `23ee5070515f6e307c59115c3dfcfa8904abbb98`
- `src/cli.rs` blob reviewed for package-owned policy resolution and fail-closed argv decoding: `49bf0180e565eb13a48ff09727157ccd829e9783`
- flags2env Rust binding: `377bff1a4e7424eb98377997a232ddc0fc700f59`

The executable harness verifies the copied `.cli-flags.toml` with the official pinned Rust binding. It proves `check-config` and `identity` are accepted as commands, that the only structured flag emitted for those command-only invocations is the binding-owned `FLAGS2ENV_COMMAND=<command>` metadata, that unexpected operands remain extras, and that credential-shaped and undeclared options fail closed.

The product's private Shared Auth dependency is intentionally outside this test: production compilation remains a separate merge gate. The product source additionally resolves package-owned policy without trusting the ambient working directory and converts `args_os()` to UTF-8 explicitly so non-Unicode argv fails as a typed error rather than panicking during `std::env::args()` iteration.
