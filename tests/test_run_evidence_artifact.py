from __future__ import annotations

import unittest

from deep_tests.run_evidence import (
    Counts,
    EvidenceValidationError,
    ProviderResult,
    ProviderStatus,
    QueueWriteStatus,
    RunEvidence,
    RunStatus,
    lima_schedule_epoch,
)
from deep_tests.run_evidence_artifact import (
    ArtifactLedger,
    RunEvidenceArtifact,
    WindowEvaluator,
    WindowState,
)

SOURCE_SHA = "1" * 40
SUITE_SHA = "2" * 40
CONFIG_DIGEST = "3" * 64
FIXTURE_DIGEST = "4" * 64
SCHEDULED = lima_schedule_epoch(2026, 8, 10)
OBSERVED = SCHEDULED + 7200


def make_evidence(
    run_id: str = "run-20260810-001",
    scheduled_at: int = SCHEDULED,
) -> RunEvidence:
    return RunEvidence(
        job="messaging-intel-contact-discovery",
        run_id=run_id,
        scheduled_at=scheduled_at,
        started_at=scheduled_at + 5,
        ended_at=scheduled_at + 30,
        source_sha=SOURCE_SHA,
        config_digest=CONFIG_DIGEST,
        fixture_digest=FIXTURE_DIGEST,
        status=RunStatus.SUCCEEDED,
        counts=Counts(
            discovered=8,
            normalized=8,
            deduplicated=7,
            suppressed=1,
            thread_checked=6,
            reviewed=6,
            accepted=5,
            rejected=1,
        ),
        providers=(
            ProviderResult("fixture-social", ProviderStatus.SUCCEEDED, 4),
            ProviderResult("fixture-web", ProviderStatus.SUCCEEDED, 4),
        ),
        queue_write_status=QueueWriteStatus.SYNTHETIC,
        outbound_enabled=False,
    )


def make_artifact(
    run_id: str = "run-20260810-001",
    scheduled_at: int = SCHEDULED,
) -> RunEvidenceArtifact:
    return RunEvidenceArtifact(make_evidence(run_id, scheduled_at), SUITE_SHA)


class RunEvidenceArtifactTests(unittest.TestCase):
    def evaluate(self, envelopes, *, scheduled_at: int = SCHEDULED, observed: int = OBSERVED):
        return WindowEvaluator().evaluate(
            job="messaging-intel-contact-discovery",
            scheduled_at=scheduled_at,
            evaluated_at=observed,
            expected_source_sha=SOURCE_SHA,
            expected_suite_sha=SUITE_SHA,
            expected_config_digest=CONFIG_DIGEST,
            envelopes=envelopes,
        )

    def test_content_addressed_artifact_round_trips(self) -> None:
        envelope = make_artifact().envelope()
        parsed = RunEvidenceArtifact.from_envelope(envelope, observed_at=OBSERVED)
        self.assertEqual(parsed.evidence.run_id, "run-20260810-001")
        self.assertEqual(parsed.suite_sha, SUITE_SHA)
        providers = envelope["evidence"]["run_evidence"]["providers"]
        self.assertEqual(providers[0]["retry_disposition"], "not-needed")

    def test_tampering_is_rejected_before_heartbeat_evaluation(self) -> None:
        envelope = make_artifact().envelope()
        envelope["evidence"]["run_evidence"]["counts"]["accepted"] = 4
        with self.assertRaisesRegex(EvidenceValidationError, "artifact digest mismatch"):
            RunEvidenceArtifact.from_envelope(envelope, observed_at=OBSERVED)

    def test_run_id_reuse_is_rejected(self) -> None:
        ledger = ArtifactLedger()
        envelope = make_artifact().envelope()
        ledger.append(envelope, observed_at=OBSERVED)
        with self.assertRaisesRegex(EvidenceValidationError, "run_id was reused"):
            ledger.append(envelope, observed_at=OBSERVED)

    def test_matching_artifact_satisfies_window(self) -> None:
        envelope = make_artifact().envelope()
        result = self.evaluate([envelope])
        self.assertEqual(result.state, WindowState.SATISFIED)
        self.assertEqual(result.final_status, "succeeded")
        self.assertEqual(result.run_id, "run-20260810-001")
        self.assertEqual(result.evidence_digest, envelope["digest"])

    def test_closed_window_without_artifact_is_missed(self) -> None:
        result = self.evaluate([])
        self.assertEqual(result.state, WindowState.MISSED)
        self.assertEqual(result.reason, "no-valid-evidence-for-window")

    def test_prior_window_artifact_is_stale_not_success(self) -> None:
        prior = make_artifact("run-20260809-001", SCHEDULED - 86400).envelope()
        result = self.evaluate([prior])
        self.assertEqual(result.state, WindowState.STALE)
        self.assertEqual(result.reason, "only-prior-window-evidence-exists")

    def test_invalid_artifact_cannot_satisfy_heartbeat(self) -> None:
        envelope = make_artifact().envelope()
        envelope["evidence"]["suite_sha"] = "5" * 40
        result = self.evaluate([envelope])
        self.assertEqual(result.state, WindowState.MISSED)
        self.assertEqual(result.reason, "invalid-evidence-rejected")
        self.assertEqual(result.invalid_evidence_count, 1)

    def test_open_reporting_window_is_pending(self) -> None:
        result = self.evaluate([], observed=SCHEDULED + 60)
        self.assertEqual(result.state, WindowState.PENDING)

    def test_mismatched_source_identity_does_not_satisfy(self) -> None:
        envelope = make_artifact().envelope()
        result = WindowEvaluator().evaluate(
            job="messaging-intel-contact-discovery",
            scheduled_at=SCHEDULED,
            evaluated_at=OBSERVED,
            expected_source_sha="9" * 40,
            expected_suite_sha=SUITE_SHA,
            expected_config_digest=CONFIG_DIGEST,
            envelopes=[envelope],
        )
        self.assertEqual(result.state, WindowState.MISSED)


if __name__ == "__main__":
    unittest.main()
