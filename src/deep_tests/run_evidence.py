from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from typing import Iterable, Mapping
from zoneinfo import ZoneInfo

EVIDENCE_SCHEMA = "messaging-intel.run-evidence.v1"
HEARTBEAT_SCHEMA = "messaging-intel.run-heartbeat.v1"
LIMA_TIMEZONE = "America/Lima"
DEFAULT_REPORTING_WINDOW_SECONDS = 4 * 60 * 60


class EvidenceValidationError(ValueError):
    pass


class RunStatus(StrEnum):
    SUCCEEDED = "succeeded"
    PARTIAL = "partial"
    FAILED = "failed"
    SKIPPED = "skipped"


class ProviderStatus(StrEnum):
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    SKIPPED = "skipped"


class FailureClass(StrEnum):
    AUTH_UNAVAILABLE = "auth_unavailable"
    CONFIGURATION = "configuration"
    INVALID_RESPONSE = "invalid_response"
    RATE_LIMITED = "rate_limited"
    TIMEOUT = "timeout"
    TRANSPORT = "transport"


class QueueWriteStatus(StrEnum):
    DISABLED = "disabled"
    SYNTHETIC = "synthetic"
    COMPLETED = "completed"
    FAILED = "failed"


class HeartbeatState(StrEnum):
    PENDING = "pending"
    SUCCEEDED = "succeeded"
    PARTIAL = "partial"
    FAILED = "failed"
    SKIPPED = "skipped"
    MISSED = "missed"


@dataclass(frozen=True)
class Counts:
    discovered: int
    normalized: int
    deduplicated: int
    suppressed: int
    thread_checked: int
    reviewed: int
    accepted: int
    rejected: int

    def validate(self) -> None:
        values = self.payload()
        if any(value < 0 for value in values.values()):
            raise EvidenceValidationError("counts must be non-negative")
        if self.normalized > self.discovered:
            raise EvidenceValidationError("normalized count exceeds discovered count")
        if self.deduplicated > self.normalized:
            raise EvidenceValidationError("deduplicated count exceeds normalized count")
        available = self.deduplicated - self.suppressed
        if available < 0:
            raise EvidenceValidationError("suppressed count exceeds deduplicated count")
        if self.thread_checked > available:
            raise EvidenceValidationError("thread-checked count exceeds unsuppressed count")
        if self.reviewed > self.thread_checked:
            raise EvidenceValidationError("reviewed count exceeds thread-checked count")
        if self.accepted + self.rejected > self.reviewed:
            raise EvidenceValidationError("accepted and rejected counts exceed reviewed count")

    def payload(self) -> dict[str, int]:
        return {
            "discovered": self.discovered,
            "normalized": self.normalized,
            "deduplicated": self.deduplicated,
            "suppressed": self.suppressed,
            "thread_checked": self.thread_checked,
            "reviewed": self.reviewed,
            "accepted": self.accepted,
            "rejected": self.rejected,
        }


@dataclass(frozen=True)
class ProviderResult:
    provider: str
    status: ProviderStatus
    discovered: int
    retries: int = 0
    failure_class: FailureClass | None = None

    def validate(self) -> None:
        if not re.fullmatch(r"[a-z][a-z0-9_-]{1,47}", self.provider):
            raise EvidenceValidationError("provider identifier is invalid")
        if self.discovered < 0 or self.retries < 0:
            raise EvidenceValidationError("provider counts must be non-negative")
        if self.status is ProviderStatus.FAILED and self.failure_class is None:
            raise EvidenceValidationError("failed provider requires a failure class")
        if self.status is not ProviderStatus.FAILED and self.failure_class is not None:
            raise EvidenceValidationError("non-failed provider cannot carry a failure class")
        if self.status is ProviderStatus.SKIPPED and self.discovered != 0:
            raise EvidenceValidationError("skipped provider cannot discover records")

    def payload(self) -> dict[str, object]:
        return {
            "provider": self.provider,
            "status": self.status.value,
            "discovered": self.discovered,
            "retries": self.retries,
            "failure_class": None if self.failure_class is None else self.failure_class.value,
        }


@dataclass(frozen=True)
class RunEvidence:
    job: str
    run_id: str
    scheduled_at: int
    started_at: int
    ended_at: int
    source_sha: str
    config_digest: str
    fixture_digest: str
    status: RunStatus
    counts: Counts
    providers: tuple[ProviderResult, ...]
    queue_write_status: QueueWriteStatus
    execution_mode: str = "dry_run"
    timezone: str = LIMA_TIMEZONE
    outbound_enabled: bool = False

    def validate(self, observed_at: int) -> None:
        if self.job != "messaging-intel-contact-discovery":
            raise EvidenceValidationError("unexpected job identifier")
        if not re.fullmatch(r"run-[a-z0-9][a-z0-9._-]{7,95}", self.run_id):
            raise EvidenceValidationError("run identifier is invalid")
        if self.timezone != LIMA_TIMEZONE:
            raise EvidenceValidationError("scheduled job timezone must be America/Lima")
        if self.execution_mode != "dry_run":
            raise EvidenceValidationError("conformance evidence must be dry-run only")
        if self.outbound_enabled:
            raise EvidenceValidationError("outbound messaging must remain disabled")
        if self.started_at < self.scheduled_at - 60:
            raise EvidenceValidationError("run started before the bounded schedule window")
        if self.ended_at < self.started_at:
            raise EvidenceValidationError("run ended before it started")
        if self.ended_at > observed_at:
            raise EvidenceValidationError("run evidence contains a future timestamp")
        if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", self.source_sha):
            raise EvidenceValidationError("source SHA is invalid")
        for label, digest in (
            ("config", self.config_digest),
            ("fixture", self.fixture_digest),
        ):
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise EvidenceValidationError(f"{label} digest is invalid")

        self.counts.validate()
        if not self.providers:
            raise EvidenceValidationError("provider coverage cannot be empty")
        names = [provider.provider for provider in self.providers]
        if len(names) != len(set(names)):
            raise EvidenceValidationError("provider identifiers must be unique")
        for provider in self.providers:
            provider.validate()
        if sum(provider.discovered for provider in self.providers) != self.counts.discovered:
            raise EvidenceValidationError("provider discoveries do not match aggregate count")

        succeeded = sum(provider.status is ProviderStatus.SUCCEEDED for provider in self.providers)
        failed = sum(provider.status is ProviderStatus.FAILED for provider in self.providers)
        if self.status is RunStatus.SUCCEEDED and (failed or not succeeded):
            raise EvidenceValidationError("succeeded run has incomplete provider success")
        if self.status is RunStatus.PARTIAL and not (succeeded and failed):
            raise EvidenceValidationError("partial run requires provider success and failure")
        if self.status is RunStatus.FAILED and (succeeded or not failed):
            raise EvidenceValidationError("failed run cannot include provider success")
        if self.status is RunStatus.SKIPPED:
            if any(self.counts.payload().values()):
                raise EvidenceValidationError("skipped run must have zero counts")
            if any(provider.status is not ProviderStatus.SKIPPED for provider in self.providers):
                raise EvidenceValidationError("skipped run requires skipped providers")

        scan_for_sensitive_material(self.body())

    def body(self) -> dict[str, object]:
        return {
            "schema": EVIDENCE_SCHEMA,
            "job": self.job,
            "run_id": self.run_id,
            "scheduled_at": self.scheduled_at,
            "started_at": self.started_at,
            "ended_at": self.ended_at,
            "timezone": self.timezone,
            "source_sha": self.source_sha,
            "config_digest": self.config_digest,
            "fixture_digest": self.fixture_digest,
            "execution_mode": self.execution_mode,
            "status": self.status.value,
            "counts": self.counts.payload(),
            "providers": [provider.payload() for provider in self.providers],
            "queue_write_status": self.queue_write_status.value,
            "outbound_enabled": self.outbound_enabled,
        }

    def canonical_body(self) -> bytes:
        return json.dumps(self.body(), sort_keys=True, separators=(",", ":")).encode()

    def digest(self) -> str:
        return "sha256:" + hashlib.sha256(self.canonical_body()).hexdigest()

    def envelope(self, observed_at: int) -> dict[str, object]:
        self.validate(observed_at)
        return {"evidence": self.body(), "digest": self.digest()}


class EvidenceLedger:
    def __init__(self) -> None:
        self._runs: dict[str, str] = {}

    def add(self, evidence: RunEvidence, envelope: Mapping[str, object], observed_at: int) -> None:
        evidence.validate(observed_at)
        expected = {"evidence": evidence.body(), "digest": evidence.digest()}
        if dict(envelope) != expected:
            raise EvidenceValidationError("evidence envelope is missing or tampered")
        if evidence.run_id in self._runs:
            raise EvidenceValidationError("run identifier was reused")
        self._runs[evidence.run_id] = evidence.digest()


@dataclass(frozen=True)
class HeartbeatResult:
    scheduled_at: int
    evaluated_at: int
    state: HeartbeatState
    run_id: str | None
    evidence_digest: str | None

    def payload(self) -> dict[str, object]:
        return {
            "schema": HEARTBEAT_SCHEMA,
            "job": "messaging-intel-contact-discovery",
            "timezone": LIMA_TIMEZONE,
            "scheduled_at": self.scheduled_at,
            "evaluated_at": self.evaluated_at,
            "state": self.state.value,
            "run_id": self.run_id,
            "evidence_digest": self.evidence_digest,
        }


class HeartbeatEvaluator:
    def __init__(self, reporting_window_seconds: int = DEFAULT_REPORTING_WINDOW_SECONDS) -> None:
        if reporting_window_seconds <= 0:
            raise ValueError("reporting window must be positive")
        self._reporting_window_seconds = reporting_window_seconds

    def evaluate(
        self,
        scheduled_at: int,
        evaluated_at: int,
        evidence: Iterable[RunEvidence],
        expected_source_sha: str,
        expected_config_digest: str,
    ) -> HeartbeatResult:
        candidates = [item for item in evidence if item.scheduled_at == scheduled_at]
        if len(candidates) > 1:
            raise EvidenceValidationError("multiple terminal evidence records exist for one schedule")
        if not candidates:
            state = (
                HeartbeatState.PENDING
                if evaluated_at <= scheduled_at + self._reporting_window_seconds
                else HeartbeatState.MISSED
            )
            return HeartbeatResult(scheduled_at, evaluated_at, state, None, None)

        item = candidates[0]
        item.validate(evaluated_at)
        if item.source_sha != expected_source_sha:
            raise EvidenceValidationError("run evidence source SHA does not match expected head")
        if item.config_digest != expected_config_digest:
            raise EvidenceValidationError("run evidence configuration digest does not match")
        state = HeartbeatState(item.status.value)
        return HeartbeatResult(
            scheduled_at=scheduled_at,
            evaluated_at=evaluated_at,
            state=state,
            run_id=item.run_id,
            evidence_digest=item.digest(),
        )


_SENSITIVE_KEY_PARTS = (
    "access_token",
    "authorization",
    "contact_name",
    "cookie",
    "email",
    "handle",
    "message_body",
    "message_text",
    "phone",
    "refresh_token",
    "secret",
)
_SENSITIVE_VALUE_PATTERNS = (
    re.compile(r"(?i)\bbearer\s+\S+"),
    re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
    re.compile(r"(?<![\w])@[A-Za-z0-9_]{3,}\b"),
    re.compile(r"\+\d{8,15}\b"),
    re.compile(r"\b\d{3}[- .]\d{3}[- .]\d{4}\b"),
    re.compile(r"\b[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}\b"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    re.compile(r"(?i)(?:access_token|refresh_token|signature|x-amz-signature)="),
)


def _walk(value: object, path: str = "$") -> list[tuple[str, object]]:
    items = [(path, value)]
    if isinstance(value, Mapping):
        for key, child in value.items():
            items.extend(_walk(child, f"{path}.{key}"))
    elif isinstance(value, (list, tuple)):
        for index, child in enumerate(value):
            items.extend(_walk(child, f"{path}[{index}]"))
    return items


def scan_for_sensitive_material(value: object) -> None:
    for path, item in _walk(value):
        lowered_path = path.lower()
        if any(part in lowered_path for part in _SENSITIVE_KEY_PARTS):
            raise EvidenceValidationError(f"sensitive field is forbidden at {path}")
        if isinstance(item, str) and any(pattern.search(item) for pattern in _SENSITIVE_VALUE_PATTERNS):
            raise EvidenceValidationError(f"sensitive value is forbidden at {path}")


def lima_schedule_epoch(year: int, month: int, day: int, hour: int = 3) -> int:
    scheduled = datetime(year, month, day, hour, 0, 0, tzinfo=ZoneInfo(LIMA_TIMEZONE))
    return int(scheduled.timestamp())
