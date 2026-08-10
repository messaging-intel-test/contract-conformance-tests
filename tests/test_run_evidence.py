import copy
import unittest

from deep_tests.run_evidence import (
    Counts,
    EvidenceLedger,
    EvidenceValidationError,
    FailureClass,
    HeartbeatEvaluator,
    HeartbeatState,
    ProviderResult,
    ProviderStatus,
    QueueWriteStatus,
    RunEvidence,
    RunStatus,
    lima_schedule_epoch,
    scan_for_sensitive_material,
)

SOURCE_SHA = "a" * 40
CONFIG_DIGEST = "b" * 64
FIXTURE_DIGEST = "c" * 64


def make_evidence(
    *,
    status: RunStatus = RunStatus.SUCCEEDED,
    run_id: str = "run-20260810-synthetic-001",
    scheduled_at: int | None = None,
) -> RunEvidence:
    scheduled = scheduled_at or lima_schedule_epoch(2026, 8, 10)
    if status is RunStatus.SUCCEEDED:
        providers = (
            ProviderResult("fixture-web", ProviderStatus.SUCCEEDED, 12),
            ProviderResult("fixture-social", ProviderStatus.SUCCEEDED, 8),
        )
        counts = Counts(20, 19, 16, 3, 11, 7, 4, 2)
    elif status is RunStatus.PARTIAL:
        providers = (
            ProviderResult("fixture-web", ProviderStatus.SUCCEEDED, 12),
            ProviderResult(
                "fixture-social",
                ProviderStatus.FAILED,
                0,
                retries=2,
                failure_class=FailureClass.TIMEOUT,
            ),
        )
        counts = Counts(12, 11, 9, 2, 6, 4, 2, 1)
    elif status is RunStatus.FAILED:
        providers = (
            ProviderResult(
                "fixture-web",
                ProviderStatus.FAILED,
                0,
                retries=1,
                failure_class=FailureClass.TRANSPORT,
            ),
            ProviderResult(
                "fixture-social",
                ProviderStatus.FAILED,
                0,
                retries=2,
                failure_class=FailureClass.TIMEOUT,
            ),
        )
        counts = Counts(0, 0, 0, 0, 0, 0, 0, 0)
    else:
        providers = (
            ProviderResult("fixture-web", ProviderStatus.SKIPPED, 0),
            ProviderResult("fixture-social", ProviderStatus.SKIPPED, 0),
        )
        counts = Counts(0, 0, 0, 0, 0, 0, 0, 0)
    return RunEvidence(
        job="messaging-intel-contact-discovery",
        run_id=run_id,
        scheduled_at=scheduled,
        started_at=scheduled + 5,
        ended_at=scheduled + 65,
        source_sha=SOURCE_SHA,
        config_digest=CONFIG_DIGEST,
        fixture_digest=FIXTURE_DIGEST,
        status=status,
        counts=counts,
        providers=providers,
        queue_write_status=QueueWriteStatus.SYNTHETIC,
    )


class RunEvidenceTests(unittest.TestCase):
    def test_success_evidence_is_canonical_content_addressed_and_redacted(self) -> None:
        evidence = make_evidence()
        observed = evidence.ended_at + 1
        first = evidence.envelope(observed)
        second = evidence.envelope(observed + 60)
        self.assertEqual(first, second)
        self.assertTrue(str(first["digest"]).startswith("sha256:"))
        serialized = evidence.canonical_body().decode()
        self.assertNotIn("phone", serialized)
        self.assertNotIn("email", serialized)
        self.assertNotIn("message", serialized)
        self.assertFalse(evidence.outbound_enabled)

    def test_partial_and_failed_provider_coverage_map_to_terminal_heartbeat(self) -> None:
        evaluator = HeartbeatEvaluator()
        for status, expected in (
            (RunStatus.PARTIAL, HeartbeatState.PARTIAL),
            (RunStatus.FAILED, HeartbeatState.FAILED),
            (RunStatus.SKIPPED, HeartbeatState.SKIPPED),
        ):
            with self.subTest(status=status):
                evidence = make_evidence(status=status, run_id=f"run-20260810-{status.value}-001")
                result = evaluator.evaluate(
                    evidence.scheduled_at,
                    evidence.ended_at + 1,
                    [evidence],
                    SOURCE_SHA,
                    CONFIG_DIGEST,
                )
                self.assertEqual(result.state, expected)
                self.assertEqual(result.run_id, evidence.run_id)
                self.assertEqual(result.evidence_digest, evidence.digest())

    def test_absence_of_evidence_is_pending_then_unambiguously_missed(self) -> None:
        scheduled = lima_schedule_epoch(2026, 8, 10)
        evaluator = HeartbeatEvaluator(reporting_window_seconds=3_600)
        pending = evaluator.evaluate(
            scheduled,
            scheduled + 3_600,
            [],
            SOURCE_SHA,
            CONFIG_DIGEST,
        )
        missed = evaluator.evaluate(
            scheduled,
            scheduled + 3_601,
            [],
            SOURCE_SHA,
            CONFIG_DIGEST,
        )
        self.assertEqual(pending.state, HeartbeatState.PENDING)
        self.assertEqual(missed.state, HeartbeatState.MISSED)
        self.assertIsNone(missed.run_id)
        self.assertIsNone(missed.evidence_digest)
        self.assertEqual(missed.payload()["state"], "missed")

    def test_tampered_envelope_and_reused_run_id_are_rejected(self) -> None:
        evidence = make_evidence()
        observed = evidence.ended_at + 1
        ledger = EvidenceLedger()
        envelope = evidence.envelope(observed)
        tampered = copy.deepcopy(envelope)
        tampered["digest"] = "sha256:" + "0" * 64
        with self.assertRaises(EvidenceValidationError):
            ledger.add(evidence, tampered, observed)
        ledger.add(evidence, envelope, observed)
        with self.assertRaises(EvidenceValidationError):
            ledger.add(evidence, envelope, observed)

    def test_future_timestamp_and_expected_head_or_config_mismatch_are_rejected(self) -> None:
        evidence = make_evidence()
        with self.assertRaises(EvidenceValidationError):
            evidence.validate(evidence.ended_at - 1)
        evaluator = HeartbeatEvaluator()
        with self.assertRaises(EvidenceValidationError):
            evaluator.evaluate(
                evidence.scheduled_at,
                evidence.ended_at + 1,
                [evidence],
                "d" * 40,
                CONFIG_DIGEST,
            )
        with self.assertRaises(EvidenceValidationError):
            evaluator.evaluate(
                evidence.scheduled_at,
                evidence.ended_at + 1,
                [evidence],
                SOURCE_SHA,
                "e" * 64,
            )

    def test_count_flow_and_provider_totals_fail_closed(self) -> None:
        scheduled = lima_schedule_epoch(2026, 8, 10)
        invalid = RunEvidence(
            job="messaging-intel-contact-discovery",
            run_id="run-20260810-invalid-counts",
            scheduled_at=scheduled,
            started_at=scheduled + 1,
            ended_at=scheduled + 2,
            source_sha=SOURCE_SHA,
            config_digest=CONFIG_DIGEST,
            fixture_digest=FIXTURE_DIGEST,
            status=RunStatus.SUCCEEDED,
            counts=Counts(5, 6, 6, 0, 0, 0, 0, 0),
            providers=(ProviderResult("fixture-web", ProviderStatus.SUCCEEDED, 5),),
            queue_write_status=QueueWriteStatus.SYNTHETIC,
        )
        with self.assertRaises(EvidenceValidationError):
            invalid.validate(scheduled + 3)

    def test_live_or_outbound_execution_is_rejected_by_conformance_contract(self) -> None:
        evidence = make_evidence()
        live = RunEvidence(**{**evidence.__dict__, "execution_mode": "live"})
        outbound = RunEvidence(**{**evidence.__dict__, "outbound_enabled": True})
        with self.assertRaises(EvidenceValidationError):
            live.validate(evidence.ended_at + 1)
        with self.assertRaises(EvidenceValidationError):
            outbound.validate(evidence.ended_at + 1)

    def test_sensitive_fields_and_values_are_rejected_without_echoing_them(self) -> None:
        samples = (
            {"phone": "+15555550123"},
            {"contact": {"email": "person@example.test"}},
            {"safe_key": "@synthetic_handle"},
            {"safe_key": "Bearer synthetic-value"},
            {"safe_key": "aaaaaaaaaaaa.bbbbbbbbbbbb.cccccccccccc"},
        )
        for sample in samples:
            with self.subTest(sample=tuple(sample)):
                with self.assertRaises(EvidenceValidationError) as raised:
                    scan_for_sensitive_material(sample)
                self.assertNotIn(str(next(iter(sample.values()))), str(raised.exception))

    def test_schedule_helper_is_exactly_three_am_lima(self) -> None:
        scheduled = lima_schedule_epoch(2026, 8, 10)
        # Lima is UTC-05 year-round; 03:00 local is 08:00 UTC.
        self.assertEqual(scheduled, 1_786_348_800)


if __name__ == "__main__":
    unittest.main()
