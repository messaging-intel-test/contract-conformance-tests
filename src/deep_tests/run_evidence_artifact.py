from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from enum import Enum
from typing import Iterable, Mapping

from .run_evidence import (
    Counts,
    EVIDENCE_SCHEMA,
    EvidenceValidationError,
    FailureClass,
    ProviderResult,
    ProviderStatus,
    QueueWriteStatus,
    RunEvidence,
    RunStatus,
    scan_for_sensitive_material,
)

ARTIFACT_SCHEMA = "messaging-intel.run-evidence-artifact.v1"
HEARTBEAT_SCHEMA = "messaging-intel.run-heartbeat.v1"
ALGORITHM = "sha256"
_SHA_40 = re.compile(r"^[0-9a-f]{40}$")
_DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")


class WindowState(str, Enum):
    PENDING = "pending"
    SATISFIED = "satisfied"
    STALE = "stale"
    MISSED = "missed"


def _artifact_digest(body: Mapping[str, object]) -> str:
    encoded = json.dumps(body, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def _provider_retry_disposition(provider: ProviderResult) -> str:
    if provider.status == ProviderStatus.SUCCEEDED:
        return "not-needed"
    if provider.status == ProviderStatus.SKIPPED:
        return "not-applicable"
    return "retry-exhausted" if provider.retries > 0 else "not-retryable"


def _provider_payload(provider: ProviderResult) -> dict[str, object]:
    payload = provider.payload()
    payload["retry_disposition"] = _provider_retry_disposition(provider)
    return payload


def _run_payload(evidence: RunEvidence) -> dict[str, object]:
    payload = evidence.body()
    payload["providers"] = [_provider_payload(provider) for provider in evidence.providers]
    return payload


def _run_evidence_from_payload(payload: Mapping[str, object]) -> RunEvidence:
    expected = {
        "config_digest",
        "counts",
        "ended_at",
        "schema",
        "execution_mode",
        "fixture_digest",
        "job",
        "outbound_enabled",
        "providers",
        "queue_write_status",
        "run_id",
        "scheduled_at",
        "source_sha",
        "started_at",
        "status",
        "timezone",
    }
    if set(payload) != expected:
        raise EvidenceValidationError("run evidence fields do not match the artifact contract")
    if payload["schema"] != EVIDENCE_SCHEMA:
        raise EvidenceValidationError("run evidence schema does not match the artifact contract")
    raw_counts = payload["counts"]
    raw_providers = payload["providers"]
    if not isinstance(raw_counts, Mapping) or not isinstance(raw_providers, list):
        raise EvidenceValidationError("run evidence counts/providers are malformed")
    count_fields = {
        "discovered",
        "normalized",
        "deduplicated",
        "suppressed",
        "thread_checked",
        "reviewed",
        "accepted",
        "rejected",
    }
    if set(raw_counts) != count_fields:
        raise EvidenceValidationError("count fields do not match the artifact contract")
    providers: list[ProviderResult] = []
    for item in raw_providers:
        if not isinstance(item, Mapping):
            raise EvidenceValidationError("provider entry must be an object")
        expected_provider = {
            "provider",
            "status",
            "discovered",
            "retries",
            "failure_class",
            "retry_disposition",
        }
        if set(item) != expected_provider:
            raise EvidenceValidationError("provider fields do not match the artifact contract")
        failure = item["failure_class"]
        provider = ProviderResult(
            provider=item["provider"],  # type: ignore[arg-type]
            status=ProviderStatus(item["status"]),  # type: ignore[arg-type]
            discovered=item["discovered"],  # type: ignore[arg-type]
            retries=item["retries"],  # type: ignore[arg-type]
            failure_class=None if failure is None else FailureClass(failure),  # type: ignore[arg-type]
        )
        if item["retry_disposition"] != _provider_retry_disposition(provider):
            raise EvidenceValidationError("provider retry disposition is inconsistent")
        providers.append(provider)
    evidence = RunEvidence(
        job=payload["job"],  # type: ignore[arg-type]
        run_id=payload["run_id"],  # type: ignore[arg-type]
        scheduled_at=payload["scheduled_at"],  # type: ignore[arg-type]
        started_at=payload["started_at"],  # type: ignore[arg-type]
        ended_at=payload["ended_at"],  # type: ignore[arg-type]
        source_sha=payload["source_sha"],  # type: ignore[arg-type]
        config_digest=payload["config_digest"],  # type: ignore[arg-type]
        fixture_digest=payload["fixture_digest"],  # type: ignore[arg-type]
        status=RunStatus(payload["status"]),  # type: ignore[arg-type]
        counts=Counts(**{key: raw_counts[key] for key in count_fields}),  # type: ignore[arg-type]
        providers=tuple(providers),
        execution_mode=payload["execution_mode"],  # type: ignore[arg-type]
        timezone=payload["timezone"],  # type: ignore[arg-type]
        queue_write_status=QueueWriteStatus(payload["queue_write_status"]),  # type: ignore[arg-type]
        outbound_enabled=payload["outbound_enabled"],  # type: ignore[arg-type]
    )
    return evidence


@dataclass(frozen=True)
class RunEvidenceArtifact:
    evidence: RunEvidence
    suite_sha: str
    schema_version: str = ARTIFACT_SCHEMA

    def validate(self, observed_at: int | None = None) -> None:
        if self.schema_version != ARTIFACT_SCHEMA:
            raise EvidenceValidationError("unsupported run-evidence artifact schema")
        if not _SHA_40.fullmatch(self.suite_sha):
            raise EvidenceValidationError("suite_sha must be a lowercase 40-character SHA")
        observed = self.evidence.ended_at if observed_at is None else observed_at
        self.evidence.validate(observed_at=observed)
        scan_for_sensitive_material(self._body_unchecked())

    def _body_unchecked(self) -> dict[str, object]:
        return {
            "run_evidence": _run_payload(self.evidence),
            "schema_version": self.schema_version,
            "suite_sha": self.suite_sha,
        }

    def body(self) -> dict[str, object]:
        self.validate()
        return self._body_unchecked()

    def envelope(self) -> dict[str, object]:
        body = self.body()
        return {
            "algorithm": ALGORITHM,
            "digest": _artifact_digest(body),
            "evidence": body,
        }

    @classmethod
    def from_envelope(
        cls,
        envelope: Mapping[str, object],
        *,
        observed_at: int | None = None,
    ) -> "RunEvidenceArtifact":
        if set(envelope) != {"algorithm", "digest", "evidence"}:
            raise EvidenceValidationError("artifact envelope fields are invalid")
        if envelope["algorithm"] != ALGORITHM:
            raise EvidenceValidationError("artifact algorithm must be sha256")
        digest = envelope["digest"]
        body = envelope["evidence"]
        if not isinstance(digest, str) or not _DIGEST.fullmatch(digest):
            raise EvidenceValidationError("artifact digest is invalid")
        if not isinstance(body, Mapping):
            raise EvidenceValidationError("artifact evidence must be an object")
        if digest != _artifact_digest(body):
            raise EvidenceValidationError("artifact digest mismatch")
        if set(body) != {"run_evidence", "schema_version", "suite_sha"}:
            raise EvidenceValidationError("artifact evidence fields are invalid")
        raw_run = body["run_evidence"]
        if not isinstance(raw_run, Mapping):
            raise EvidenceValidationError("run_evidence must be an object")
        artifact = cls(
            evidence=_run_evidence_from_payload(raw_run),
            suite_sha=body["suite_sha"],  # type: ignore[arg-type]
            schema_version=body["schema_version"],  # type: ignore[arg-type]
        )
        artifact.validate(observed_at=observed_at)
        return artifact


class ArtifactLedger:
    def __init__(self) -> None:
        self._run_ids: set[str] = set()
        self._digests: set[str] = set()

    def append(
        self,
        envelope: Mapping[str, object],
        *,
        observed_at: int | None = None,
    ) -> RunEvidenceArtifact:
        artifact = RunEvidenceArtifact.from_envelope(envelope, observed_at=observed_at)
        digest = envelope["digest"]
        assert isinstance(digest, str)
        if artifact.evidence.run_id in self._run_ids:
            raise EvidenceValidationError("run_id was reused")
        if digest in self._digests:
            raise EvidenceValidationError("artifact digest was reused")
        self._run_ids.add(artifact.evidence.run_id)
        self._digests.add(digest)
        return artifact


@dataclass(frozen=True)
class WindowResult:
    job: str
    scheduled_at: int
    evaluated_at: int
    state: WindowState
    reason: str
    invalid_evidence_count: int
    evidence_digest: str | None = None
    final_status: str | None = None
    run_id: str | None = None
    schema_version: str = HEARTBEAT_SCHEMA

    def payload(self) -> dict[str, object]:
        if self.state == WindowState.SATISFIED:
            if self.evidence_digest is None or self.final_status is None or self.run_id is None:
                raise EvidenceValidationError("satisfied heartbeat requires exact evidence identity")
        elif self.evidence_digest is not None or self.final_status is not None or self.run_id is not None:
            raise EvidenceValidationError("non-satisfied heartbeat cannot claim run evidence")
        return {
            "evaluated_at": self.evaluated_at,
            "evidence_digest": self.evidence_digest,
            "final_status": self.final_status,
            "invalid_evidence_count": self.invalid_evidence_count,
            "job": self.job,
            "reason": self.reason,
            "run_id": self.run_id,
            "scheduled_at": self.scheduled_at,
            "schema_version": self.schema_version,
            "state": self.state.value,
            "timezone": "America/Lima",
        }


class WindowEvaluator:
    def __init__(self, reporting_window_seconds: int = 3600) -> None:
        if reporting_window_seconds <= 0:
            raise ValueError("reporting_window_seconds must be positive")
        self.reporting_window_seconds = reporting_window_seconds

    def evaluate(
        self,
        *,
        job: str,
        scheduled_at: int,
        evaluated_at: int,
        expected_source_sha: str,
        expected_suite_sha: str,
        expected_config_digest: str,
        envelopes: Iterable[Mapping[str, object]],
    ) -> WindowResult:
        if evaluated_at <= scheduled_at + self.reporting_window_seconds:
            return WindowResult(
                job=job,
                scheduled_at=scheduled_at,
                evaluated_at=evaluated_at,
                state=WindowState.PENDING,
                reason="reporting-window-open",
                invalid_evidence_count=0,
            )

        valid: list[tuple[RunEvidenceArtifact, str]] = []
        invalid_count = 0
        for envelope in envelopes:
            try:
                artifact = RunEvidenceArtifact.from_envelope(envelope, observed_at=evaluated_at)
            except (EvidenceValidationError, TypeError, ValueError):
                invalid_count += 1
                continue
            digest = envelope["digest"]
            assert isinstance(digest, str)
            valid.append((artifact, digest))

        matching: list[tuple[RunEvidenceArtifact, str]] = []
        prior: list[RunEvidenceArtifact] = []
        for artifact, digest in valid:
            evidence = artifact.evidence
            identity_matches = (
                evidence.job == job
                and evidence.source_sha == expected_source_sha
                and artifact.suite_sha == expected_suite_sha
                and evidence.config_digest == expected_config_digest
            )
            if not identity_matches:
                continue
            if evidence.scheduled_at == scheduled_at:
                matching.append((artifact, digest))
            elif evidence.scheduled_at < scheduled_at:
                prior.append(artifact)

        if len(matching) > 1:
            raise EvidenceValidationError("multiple artifacts claim one schedule window")
        if matching:
            artifact, digest = matching[0]
            return WindowResult(
                job=job,
                scheduled_at=scheduled_at,
                evaluated_at=evaluated_at,
                state=WindowState.SATISFIED,
                reason="valid-evidence-inside-window",
                invalid_evidence_count=invalid_count,
                evidence_digest=digest,
                final_status=artifact.evidence.status.value,
                run_id=artifact.evidence.run_id,
            )
        if prior:
            return WindowResult(
                job=job,
                scheduled_at=scheduled_at,
                evaluated_at=evaluated_at,
                state=WindowState.STALE,
                reason="only-prior-window-evidence-exists",
                invalid_evidence_count=invalid_count,
            )
        return WindowResult(
            job=job,
            scheduled_at=scheduled_at,
            evaluated_at=evaluated_at,
            state=WindowState.MISSED,
            reason="invalid-evidence-rejected" if invalid_count else "no-valid-evidence-for-window",
            invalid_evidence_count=invalid_count,
        )
