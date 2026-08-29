# messaging-intel-test/contract-conformance-tests

Deterministic state-model, idempotency, serialization, and protocol contract conformance tests.

This repository is the `contract` deep-test suite for `messaging-intel`. It is intentionally dependency-light and deterministic so failures can be reproduced locally without production credentials or customer data.

## Run

```bash
PYTHONPATH=src python -m unittest discover -s tests -v
python scripts/verify_repository.py
```

## Contract slices

* The original reference store proves deterministic replay, canonical serialization, tombstones, and idempotency conflict handling.
* [`docs/nightly-run-evidence-contract.md`](docs/nightly-run-evidence-contract.md) proves content-addressed redacted run evidence, provider/count invariants, exact-head/config binding, tamper and run-ID replay rejection, the 03:00 America/Lima schedule, and a fail-closed pending-to-missed heartbeat.

Product adapters should be added through focused pull requests while preserving the reference-model tests as an oracle. Email and dashboard reporting must consume validated evidence; workflow existence or absence of logged errors is not proof of success.

Tracking: https://github.com/ORESoftware/ai-agent-coordinator.rs/issues/139
