use ores_common_desktop_cli::{is_full_git_commit_sha, DeployRequest, DocsGenerationMode};
use ores_common_desktop_infra::{
    is_obviously_mutable_revision, is_sha256, validate_desired_state, DesiredState, RouteAuthority,
    RouteChange,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_AUTH_ISSUER: &str = "https://ores-shared-auth.com";
pub const OTEL_EVENT_PREFIX: &str = "ores.desktop";
pub const READ_SCOPE: &str = "ores.desktop.read";
pub const LIFECYCLE_SCOPE: &str = "ores.desktop.lifecycle";
pub const UPDATE_SCOPE: &str = "ores.desktop.update";
pub const ROLLBACK_SCOPE: &str = "ores.desktop.rollback";
pub const DEPLOY_SCOPE: &str = "ores.desktop.deploy";
pub const MAX_DRAIN_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    Doctor,
    Status,
    Plan,
    Up,
    Down,
    Reload,
    Update,
    Rollback,
    Routes,
    Logs,
    Deploy,
}

impl Transition {
    pub fn required_scope(&self) -> &'static str {
        return match self {
            Self::Doctor | Self::Status | Self::Plan | Self::Routes | Self::Logs => READ_SCOPE,
            Self::Up | Self::Down | Self::Reload => LIFECYCLE_SCOPE,
            Self::Update => UPDATE_SCOPE,
            Self::Rollback => ROLLBACK_SCOPE,
            Self::Deploy => DEPLOY_SCOPE,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStage {
    Staged,
    Verified,
    HotReloading,
    Restarting,
    HealthChecking,
    Draining,
    Committed,
    RollingBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStage {
    SourceResolved,
    ComposeValidated,
    ContractsDiscovered,
    DocsGenerated,
    WorkersStaged,
    RoutesPrepared,
    Activated,
    OldGenerationDraining,
    Committed,
    RollingBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerActivation {
    Reuse,
    StartChild,
    RestartChild,
    BeamHotLoad,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDeployment {
    pub service_id: String,
    pub revision: String,
    pub activation: WorkerActivation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsArtifactPlan {
    pub api_docs_revision: String,
    pub publication_mode: String,
    pub semantic_contract_sha256: String,
    pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub deployment_id: String,
    pub product_id: String,
    pub generation: u64,
    pub route_catalog_sha256: String,
    pub workers: Vec<WorkerDeployment>,
    pub route_changes: Vec<RouteChange>,
    pub docs: Option<DocsArtifactPlan>,
    pub full_appliance_restart_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentActivationReceipt {
    pub deployment_id: String,
    pub product_id: String,
    pub generation: u64,
    pub route_catalog_sha256: String,
    pub workers_staged: bool,
    pub health_checked: bool,
    pub routes_committed: bool,
    pub old_generation_drained: bool,
    pub state_committed: bool,
    pub rollback_retained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Desktop,
    Android,
    Ios,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostingRole {
    ClientOnly,
    ClientAndWorker,
    ClientAndServer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    pub platform: DevicePlatform,
    pub hosting_role: HostingRole,
    pub supports_wasm: bool,
    pub supports_native_workers: bool,
    pub supports_persistent_service: bool,
    pub supports_inbound_listener: bool,
    pub background_execution_limited: bool,
}

impl DeviceCapabilities {
    pub fn can_host_worker(&self) -> bool {
        if self.hosting_role == HostingRole::ClientOnly {
            return false;
        }

        return self.supports_wasm || self.supports_native_workers;
    }

    pub fn can_host_persistent_backend(&self) -> bool {
        if self.hosting_role != HostingRole::ClientAndServer {
            return false;
        }

        if self.background_execution_limited {
            return false;
        }

        return self.supports_persistent_service;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonPolicy {
    pub product_id: String,
    pub listen_addr: SocketAddr,
    pub token_file: PathBuf,
    pub auth_issuer: String,
    pub allowed_transitions: BTreeSet<Transition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedPrincipal {
    pub subject: String,
    pub device_id: Option<String>,
    pub scopes: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateComponent {
    pub component_id: String,
    pub from_revision: String,
    pub to_revision: String,
    pub expected_digest: Option<String>,
    pub hot_reloadable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePlan {
    pub update_id: String,
    pub components: Vec<UpdateComponent>,
    pub drain_timeout_ms: u64,
}

impl UpdatePlan {
    pub fn drain_timeout(&self) -> Duration {
        return Duration::from_millis(self.drain_timeout_ms);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub event_name: String,
    pub product_id: String,
    pub generation: Option<u64>,
    pub update_id: Option<String>,
    pub update_stage: Option<UpdateStage>,
    pub deployment_id: Option<String>,
    pub deployment_stage: Option<DeploymentStage>,
}

pub trait TelemetrySink: Send + Sync {
    fn emit(&self, event: TelemetryEvent);
}

pub trait ProductDaemonAdapter {
    fn product_id(&self) -> &str;
    fn apply_transition(&self, transition: &Transition, desired_state: &DesiredState) -> Result<(), String>;
}

pub trait ProductDeploymentAdapter {
    fn product_id(&self) -> &str;
    fn plan_deployment(&self, request: &DeployRequest, generation: u64) -> Result<DeploymentPlan, String>;
    fn activate_deployment(&self, plan: &DeploymentPlan) -> Result<DeploymentActivationReceipt, String>;
    fn rollback_deployment(&self, plan: &DeploymentPlan) -> Result<(), String>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("daemon product_id must not be empty")]
    EmptyProductId,
    #[error("daemon must bind to a non-zero loopback socket")]
    InvalidLoopbackBind,
    #[error("token_file is required")]
    MissingTokenFile,
    #[error("auth issuer must be an https URL")]
    InvalidAuthIssuer,
    #[error("authenticated principal subject is required")]
    MissingPrincipalSubject,
    #[error("authenticated device_id is required")]
    MissingDeviceId,
    #[error("transition is not allowed by daemon policy: {0:?}")]
    TransitionDenied(Transition),
    #[error("deploy must use execute_deployment rather than the generic transition path")]
    DeployRequiresDeploymentContract,
    #[error("update_id must not be empty")]
    EmptyUpdateId,
    #[error("update plan must contain at least one component")]
    EmptyUpdatePlan,
    #[error("duplicate update component: {0}")]
    DuplicateUpdateComponent(String),
    #[error("update component must use exact non-empty revisions: {0}")]
    MutableRevision(String),
    #[error("update component requires a valid sha256 artifact digest: {0}")]
    InvalidUpdateDigest(String),
    #[error("drain timeout must be between 1ms and 24h")]
    InvalidDrainTimeout,
    #[error("principal is missing required scope: {0}")]
    MissingScope(String),
    #[error("product identity mismatch: expected {expected}, got {actual}")]
    ProductIdMismatch { expected: String, actual: String },
    #[error("product transition failed: {0}")]
    ProductTransition(String),
    #[error("invalid deploy request: {0}")]
    InvalidDeployRequest(String),
    #[error("deployment planning failed: {0}")]
    DeploymentPlanning(String),
    #[error("deployment activation failed: {0}")]
    DeploymentActivation(String),
    #[error("deployment activation failed ({activation}) and rollback failed ({rollback})")]
    DeploymentActivationRollback { activation: String, rollback: String },
    #[error("deployment receipt is invalid: {0}")]
    InvalidDeploymentReceipt(String),
    #[error("deployment receipt was invalid ({receipt}) and rollback failed ({rollback})")]
    DeploymentReceiptRollback { receipt: String, rollback: String },
    #[error("ordinary application deployment must not restart the complete desktop appliance")]
    UnexpectedApplianceRestart,
    #[error("deployment_id must not be empty")]
    EmptyDeploymentId,
    #[error("deployment generation mismatch: expected {expected}, got {actual}")]
    GenerationMismatch { expected: u64, actual: u64 },
    #[error("deployment route catalog must have a valid sha256 digest")]
    InvalidRouteCatalogDigest,
    #[error("duplicate deployment worker: {0}")]
    DuplicateWorker(String),
    #[error("deployment worker must use an exact revision: {0}")]
    MutableWorkerRevision(String),
    #[error("deployment route change is invalid: {0}")]
    InvalidRouteChange(String),
    #[error("duplicate route change for route_id: {0}")]
    DuplicateRouteChange(String),
    #[error("consumer-owned deterministic docs require a pinned api-docs revision")]
    MissingApiDocsRevision,
    #[error("deployment docs must use consumer_owned publication mode")]
    InvalidDocsPublicationMode,
    #[error("deployment docs semantic digest must equal the live route catalog digest")]
    DocsDigestMismatch,
    #[error("deployment docs output_dir must not be empty")]
    MissingDocsOutputDir,
    #[error("docs artifact must be absent when docs generation is disabled")]
    UnexpectedDocsArtifact,
}

pub fn validate_policy(policy: &DaemonPolicy) -> Result<(), PolicyError> {
    if policy.product_id.trim().is_empty() {
        return Err(PolicyError::EmptyProductId);
    }

    if !policy.listen_addr.ip().is_loopback() || policy.listen_addr.port() == 0 {
        return Err(PolicyError::InvalidLoopbackBind);
    }

    if policy.token_file.as_os_str().is_empty() {
        return Err(PolicyError::MissingTokenFile);
    }

    if !is_https_issuer(&policy.auth_issuer) {
        return Err(PolicyError::InvalidAuthIssuer);
    }

    return Ok(());
}

pub fn validate_principal(principal: &AuthenticatedPrincipal) -> Result<(), PolicyError> {
    if principal.subject.trim().is_empty() {
        return Err(PolicyError::MissingPrincipalSubject);
    }

    if principal
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|device_id| !device_id.is_empty())
        .is_none()
    {
        return Err(PolicyError::MissingDeviceId);
    }

    return Ok(());
}

pub fn authorize_transition(policy: &DaemonPolicy, transition: &Transition) -> Result<(), PolicyError> {
    if !policy.allowed_transitions.contains(transition) {
        return Err(PolicyError::TransitionDenied(transition.clone()));
    }

    return Ok(());
}

pub fn authorize_scope(principal: &AuthenticatedPrincipal, required_scope: &str) -> Result<(), PolicyError> {
    validate_principal(principal)?;

    if !principal.scopes.contains(required_scope) {
        return Err(PolicyError::MissingScope(required_scope.to_string()));
    }

    return Ok(());
}

pub fn validate_update_plan(plan: &UpdatePlan) -> Result<(), PolicyError> {
    if plan.update_id.trim().is_empty() {
        return Err(PolicyError::EmptyUpdateId);
    }

    if plan.components.is_empty() {
        return Err(PolicyError::EmptyUpdatePlan);
    }

    if plan.drain_timeout_ms == 0 || plan.drain_timeout_ms > MAX_DRAIN_TIMEOUT_MS {
        return Err(PolicyError::InvalidDrainTimeout);
    }

    let mut component_ids = BTreeSet::new();

    for component in &plan.components {
        if component.component_id.trim().is_empty() || !component_ids.insert(component.component_id.clone()) {
            return Err(PolicyError::DuplicateUpdateComponent(
                component.component_id.clone(),
            ));
        }

        if is_obviously_mutable_revision(&component.from_revision)
            || is_obviously_mutable_revision(&component.to_revision)
        {
            return Err(PolicyError::MutableRevision(component.component_id.clone()));
        }

        let digest = component
            .expected_digest
            .as_deref()
            .ok_or_else(|| PolicyError::InvalidUpdateDigest(component.component_id.clone()))?;

        if !is_sha256(digest) {
            return Err(PolicyError::InvalidUpdateDigest(
                component.component_id.clone(),
            ));
        }
    }

    return Ok(());
}

pub fn validate_deployment_plan(
    request: &DeployRequest,
    plan: &DeploymentPlan,
    expected_generation: u64,
) -> Result<(), PolicyError> {
    request
        .validate()
        .map_err(|error| PolicyError::InvalidDeployRequest(error.to_string()))?;

    if plan.deployment_id.trim().is_empty() {
        return Err(PolicyError::EmptyDeploymentId);
    }

    if plan.generation != expected_generation || plan.generation == 0 {
        return Err(PolicyError::GenerationMismatch {
            expected: expected_generation,
            actual: plan.generation,
        });
    }

    if !is_sha256(&plan.route_catalog_sha256) {
        return Err(PolicyError::InvalidRouteCatalogDigest);
    }

    if plan.full_appliance_restart_required {
        return Err(PolicyError::UnexpectedApplianceRestart);
    }

    let mut worker_ids = BTreeSet::new();

    for worker in &plan.workers {
        if worker.service_id.trim().is_empty() || !worker_ids.insert(worker.service_id.clone()) {
            return Err(PolicyError::DuplicateWorker(worker.service_id.clone()));
        }

        if is_obviously_mutable_revision(&worker.revision) {
            return Err(PolicyError::MutableWorkerRevision(worker.service_id.clone()));
        }
    }

    validate_route_changes(plan)?;

    match request.docs_generation {
        DocsGenerationMode::ConsumerOwned => {
            let docs = plan.docs.as_ref().ok_or(PolicyError::MissingApiDocsRevision)?;

            if !is_full_git_commit_sha(&docs.api_docs_revision) {
                return Err(PolicyError::MissingApiDocsRevision);
            }

            if docs.publication_mode != "consumer_owned" {
                return Err(PolicyError::InvalidDocsPublicationMode);
            }

            if !is_sha256(&docs.semantic_contract_sha256)
                || docs.semantic_contract_sha256 != plan.route_catalog_sha256
            {
                return Err(PolicyError::DocsDigestMismatch);
            }

            if docs.output_dir.as_os_str().is_empty() {
                return Err(PolicyError::MissingDocsOutputDir);
            }
        }
        DocsGenerationMode::Disabled => {
            if plan.docs.is_some() {
                return Err(PolicyError::UnexpectedDocsArtifact);
            }
        }
    }

    return Ok(());
}

pub fn validate_activation_receipt(
    plan: &DeploymentPlan,
    receipt: &DeploymentActivationReceipt,
) -> Result<(), PolicyError> {
    if receipt.deployment_id != plan.deployment_id
        || receipt.product_id != plan.product_id
        || receipt.generation != plan.generation
        || receipt.route_catalog_sha256 != plan.route_catalog_sha256
    {
        return Err(PolicyError::InvalidDeploymentReceipt(
            "identity, generation, or route digest mismatch".to_string(),
        ));
    }

    if !receipt.workers_staged
        || !receipt.health_checked
        || !receipt.routes_committed
        || !receipt.old_generation_drained
        || !receipt.state_committed
        || !receipt.rollback_retained
    {
        return Err(PolicyError::InvalidDeploymentReceipt(
            "activation receipt did not prove every transaction boundary".to_string(),
        ));
    }

    return Ok(());
}

pub fn execute_transition<A: ProductDaemonAdapter>(
    policy: &DaemonPolicy,
    principal: &AuthenticatedPrincipal,
    adapter: &A,
    transition: Transition,
    desired_state: &DesiredState,
) -> Result<(), PolicyError> {
    validate_policy(policy)?;

    if transition == Transition::Deploy {
        return Err(PolicyError::DeployRequiresDeploymentContract);
    }

    authorize_transition(policy, &transition)?;
    authorize_scope(principal, transition.required_scope())?;

    if policy.product_id != adapter.product_id() {
        return Err(PolicyError::ProductIdMismatch {
            expected: policy.product_id.clone(),
            actual: adapter.product_id().to_string(),
        });
    }

    if desired_state.product_id != policy.product_id {
        return Err(PolicyError::ProductIdMismatch {
            expected: policy.product_id.clone(),
            actual: desired_state.product_id.clone(),
        });
    }

    validate_desired_state(desired_state)
        .map_err(|error| PolicyError::ProductTransition(error.to_string()))?;
    adapter
        .apply_transition(&transition, desired_state)
        .map_err(PolicyError::ProductTransition)?;

    return Ok(());
}

pub fn execute_deployment<A: ProductDeploymentAdapter>(
    policy: &DaemonPolicy,
    principal: &AuthenticatedPrincipal,
    adapter: &A,
    request: &DeployRequest,
    generation: u64,
) -> Result<DeploymentPlan, PolicyError> {
    validate_policy(policy)?;
    authorize_transition(policy, &Transition::Deploy)?;
    authorize_scope(principal, DEPLOY_SCOPE)?;

    if generation == 0 {
        return Err(PolicyError::GenerationMismatch {
            expected: 1,
            actual: 0,
        });
    }

    request
        .validate()
        .map_err(|error| PolicyError::InvalidDeployRequest(error.to_string()))?;

    if policy.product_id != adapter.product_id() {
        return Err(PolicyError::ProductIdMismatch {
            expected: policy.product_id.clone(),
            actual: adapter.product_id().to_string(),
        });
    }

    let plan = adapter
        .plan_deployment(request, generation)
        .map_err(PolicyError::DeploymentPlanning)?;

    if plan.product_id != policy.product_id {
        return Err(PolicyError::ProductIdMismatch {
            expected: policy.product_id.clone(),
            actual: plan.product_id.clone(),
        });
    }

    validate_deployment_plan(request, &plan, generation)?;

    let receipt = match adapter.activate_deployment(&plan) {
        Ok(receipt) => receipt,
        Err(activation) => {
            return match adapter.rollback_deployment(&plan) {
                Ok(()) => Err(PolicyError::DeploymentActivation(activation)),
                Err(rollback) => Err(PolicyError::DeploymentActivationRollback {
                    activation,
                    rollback,
                }),
            };
        }
    };

    if let Err(receipt_error) = validate_activation_receipt(&plan, &receipt) {
        let receipt_message = receipt_error.to_string();

        return match adapter.rollback_deployment(&plan) {
            Ok(()) => Err(receipt_error),
            Err(rollback) => Err(PolicyError::DeploymentReceiptRollback {
                receipt: receipt_message,
                rollback,
            }),
        };
    }

    return Ok(plan);
}

fn validate_route_changes(plan: &DeploymentPlan) -> Result<(), PolicyError> {
    let mut route_ids = BTreeSet::new();

    for change in &plan.route_changes {
        let route_id = match change {
            RouteChange::Add(route) | RouteChange::Replace(route) => {
                let state = DesiredState {
                    product_id: plan.product_id.clone(),
                    generation: plan.generation,
                    route_authority: RouteAuthority::Rust,
                    routes: vec![route.clone()],
                    services: vec![],
                    labels: BTreeMap::new(),
                };

                validate_desired_state(&state)
                    .map_err(|error| PolicyError::InvalidRouteChange(error.to_string()))?;
                route.route_id.clone()
            }
            RouteChange::Remove { route_id } => {
                if route_id.trim().is_empty() {
                    return Err(PolicyError::InvalidRouteChange(
                        "remove route_id must not be empty".to_string(),
                    ));
                }

                route_id.clone()
            }
        };

        if !route_ids.insert(route_id.clone()) {
            return Err(PolicyError::DuplicateRouteChange(route_id));
        }
    }

    return Ok(());
}

fn is_https_issuer(value: &str) -> bool {
    let value = value.trim();

    return value.starts_with("https://")
        && value.len() > "https://".len()
        && !value.chars().any(char::is_whitespace);
}

pub fn planned_generation_event(state: &DesiredState) -> TelemetryEvent {
    return TelemetryEvent {
        event_name: format!("{OTEL_EVENT_PREFIX}.desired_state.planned"),
        product_id: state.product_id.clone(),
        generation: Some(state.generation),
        update_id: None,
        update_stage: None,
        deployment_id: None,
        deployment_stage: None,
    };
}

pub fn update_stage_event(product_id: &str, update_id: &str, stage: UpdateStage) -> TelemetryEvent {
    return TelemetryEvent {
        event_name: format!("{OTEL_EVENT_PREFIX}.update.stage"),
        product_id: product_id.to_string(),
        generation: None,
        update_id: Some(update_id.to_string()),
        update_stage: Some(stage),
        deployment_id: None,
        deployment_stage: None,
    };
}

pub fn deployment_stage_event(
    product_id: &str,
    deployment_id: &str,
    generation: u64,
    stage: DeploymentStage,
) -> TelemetryEvent {
    return TelemetryEvent {
        event_name: format!("{OTEL_EVENT_PREFIX}.deployment.stage"),
        product_id: product_id.to_string(),
        generation: Some(generation),
        update_id: None,
        update_stage: None,
        deployment_id: Some(deployment_id.to_string()),
        deployment_stage: Some(stage),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use ores_common_desktop_cli::{DeploySource, RouteDiscoveryMode};
    use std::net::{IpAddr, Ipv4Addr};

    const API_DOCS_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const ROUTE_SHA: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn policy(transitions: BTreeSet<Transition>) -> DaemonPolicy {
        return DaemonPolicy {
            product_id: "scintilla".to_string(),
            listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8765),
            token_file: PathBuf::from("/tmp/scintilla.token"),
            auth_issuer: DEFAULT_AUTH_ISSUER.to_string(),
            allowed_transitions: transitions,
        };
    }

    fn principal(scopes: &[&str]) -> AuthenticatedPrincipal {
        return AuthenticatedPrincipal {
            subject: "user-1".to_string(),
            device_id: Some("device-1".to_string()),
            scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        };
    }

    fn state() -> DesiredState {
        return DesiredState {
            product_id: "scintilla".to_string(),
            generation: 2,
            route_authority: RouteAuthority::Erlang,
            routes: vec![],
            services: vec![],
            labels: BTreeMap::new(),
        };
    }

    #[test]
    fn remote_or_zero_bind_is_rejected() {
        let mut remote_policy = policy(BTreeSet::new());
        remote_policy.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8765);
        assert_eq!(validate_policy(&remote_policy), Err(PolicyError::InvalidLoopbackBind));

        let mut zero_port_policy = policy(BTreeSet::new());
        zero_port_policy.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        assert_eq!(validate_policy(&zero_port_policy), Err(PolicyError::InvalidLoopbackBind));
    }

    #[test]
    fn mutation_requires_the_transition_specific_scope() {
        let policy = policy(BTreeSet::from([Transition::Update]));

        assert_eq!(
            authorize_scope(&principal(&[READ_SCOPE]), Transition::Update.required_scope()),
            Err(PolicyError::MissingScope(UPDATE_SCOPE.to_string()))
        );
    }

    #[test]
    fn principal_requires_device_identity() {
        let principal = AuthenticatedPrincipal {
            subject: "user-1".to_string(),
            device_id: None,
            scopes: BTreeSet::from([READ_SCOPE.to_string()]),
        };

        assert_eq!(validate_principal(&principal), Err(PolicyError::MissingDeviceId));
    }

    #[test]
    fn hosting_is_capability_negotiated_not_platform_assumed() {
        let constrained_mobile = DeviceCapabilities {
            platform: DevicePlatform::Ios,
            hosting_role: HostingRole::ClientAndServer,
            supports_wasm: true,
            supports_native_workers: false,
            supports_persistent_service: true,
            supports_inbound_listener: false,
            background_execution_limited: true,
        };

        assert!(constrained_mobile.can_host_worker());
        assert!(!constrained_mobile.can_host_persistent_backend());
    }

    #[test]
    fn ordinary_app_deploy_cannot_restart_entire_appliance() {
        let request = DeployRequest {
            source: DeploySource::LocalFolder {
                path: PathBuf::from("/work/app"),
            },
            compose_file: ".ores-compose.yaml".to_string(),
            route_discovery: RouteDiscoveryMode::ComposeAndBuildMetadata,
            docs_generation: DocsGenerationMode::Disabled,
            api_docs_revision: None,
        };
        let plan = DeploymentPlan {
            deployment_id: "deploy-1".to_string(),
            product_id: "scintilla".to_string(),
            generation: 3,
            route_catalog_sha256: ROUTE_SHA.to_string(),
            workers: vec![],
            route_changes: vec![],
            docs: None,
            full_appliance_restart_required: true,
        };

        assert_eq!(
            validate_deployment_plan(&request, &plan, 3),
            Err(PolicyError::UnexpectedApplianceRestart)
        );
    }

    #[test]
    fn consumer_docs_must_match_live_route_digest() {
        let request = DeployRequest::canonical(
            DeploySource::LocalFolder {
                path: PathBuf::from("/work/app"),
            },
            API_DOCS_SHA,
        );
        let plan = DeploymentPlan {
            deployment_id: "deploy-1".to_string(),
            product_id: "scintilla".to_string(),
            generation: 3,
            route_catalog_sha256: ROUTE_SHA.to_string(),
            workers: vec![],
            route_changes: vec![],
            docs: Some(DocsArtifactPlan {
                api_docs_revision: API_DOCS_SHA.to_string(),
                publication_mode: "consumer_owned".to_string(),
                semantic_contract_sha256: "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
                output_dir: PathBuf::from("/tmp/docs"),
            }),
            full_appliance_restart_required: false,
        };

        assert_eq!(
            validate_deployment_plan(&request, &plan, 3),
            Err(PolicyError::DocsDigestMismatch)
        );
    }

    struct TestDaemonAdapter;

    impl ProductDaemonAdapter for TestDaemonAdapter {
        fn product_id(&self) -> &str {
            return "scintilla";
        }

        fn apply_transition(&self, _transition: &Transition, _desired_state: &DesiredState) -> Result<(), String> {
            return Ok(());
        }
    }

    #[test]
    fn product_daemon_entrypoint_requires_scoped_principal() {
        let policy = policy(BTreeSet::from([Transition::Reload]));

        assert_eq!(
            execute_transition(
                &policy,
                &principal(&[LIFECYCLE_SCOPE]),
                &TestDaemonAdapter,
                Transition::Reload,
                &state(),
            ),
            Ok(())
        );
    }
}
