# Nightly discovery run-evidence and heartbeat contract

Tracking: [DEN-3425](https://linear.app/denman/issue/DEN-3425/messaging-intel-test-add-fail-closed-nightly-run-evidence-and-stale)

This suite models evidence for the Messaging Intel contact-discovery job scheduled at 03:00 `America/Lima`. It uses synthetic provider fixtures only and cannot enable outbound messaging.

## Evidence invariants

* Each terminal run binds one run ID to the exact source SHA, configuration digest, fixture digest, schedule, timestamps, provider coverage, aggregate counts, queue-write disposition, and terminal status.
* Evidence is canonical JSON and content-addressed with SHA-256.
* Provider-level discoveries must equal the aggregate discovered count. Normalization, deduplication, suppression, thread-check, review, acceptance, and rejection counts follow a fail-closed monotonic flow.
* Provider failures use a bounded failure-class enum; raw provider payloads and errors are not accepted.
* Reused run IDs, altered envelopes, future timestamps, wrong source heads, wrong configuration digests, duplicate terminal records, and inconsistent status/provider combinations are rejected.
* The conformance lane is always `dry_run`, and `outbound_enabled` must remain false.
* Phone numbers, email addresses, handles, messages, cookies, authorization material, access/refresh tokens, private keys, JWT-like values, and signed query parameters are forbidden in evidence.

## Heartbeat behavior

The heartbeat is `pending` through the reporting-window deadline. With no valid terminal evidence after that deadline, it becomes exactly `missed`; absence of evidence can never become success. Valid evidence maps to `succeeded`, `partial`, `failed`, or `skipped` and carries the evidence digest.

## Run

```bash
PYTHONPATH=src python -m unittest tests.test_run_evidence -v
```

Email or dashboard adapters should consume the heartbeat payload rather than infer success from workflow existence, a scheduled timestamp, or an empty error log.
