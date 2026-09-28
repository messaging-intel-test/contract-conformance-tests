use ores_common_desktop_cli::{DeployRequest, DocsGenerationMode};
use ores_common_desktop_infra::{DesiredState, RouteChange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_AUTH_ISSUER: &str = "https://ores-shared-auth.com";
pub const OTEL_EVENT_PREFIX: &str = "ores.desktop";
pub const DEPLOY_SCOPE: &str = "ores.desktop.deploy";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition { Doctor, Status, Plan, Up, Down, Reload, Update, Rollback, Routes, Logs, Deploy }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStage { Staged, Verified, HotReloading, Restarting, HealthChecking, Draining, Committed, RollingBack }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStage { SourceResolved, ComposeValidated, ContractsDiscovered, DocsGenerated, WorkersStaged, RoutesPrepared, Activated, OldGenerationDraining, Committed, RollingBack }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerActivation { Reuse, StartChild, RestartChild, BeamHotLoad }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDeployment { pub service_id: String, pub revision: String, pub activation: WorkerActivation }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsArtifactPlan { pub api_docs_revision: String, pub publication_mode: String, pub semantic_contract_sha256: String, pub output_dir: PathBuf }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPlan { pub deployment_id: String, pub product_id: String, pub generation: u64, pub workers: Vec<WorkerDeployment>, pub route_changes: Vec<RouteChange>, pub docs: Option<DocsArtifactPlan>, pub full_appliance_restart_required: bool }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform { Desktop, Android, Ios }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostingRole { ClientOnly, ClientAndWorker, ClientAndServer }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapabilities { pub platform: DevicePlatform, pub hosting_role: HostingRole, pub supports_wasm: bool, pub supports_native_workers: bool, pub supports_persistent_service: bool, pub supports_inbound_listener: bool, pub background_execution_limited: bool }
impl DeviceCapabilities { pub fn can_host_worker(&self) -> bool { if self.hosting_role == HostingRole::ClientOnly { return false; } return self.supports_wasm || self.supports_native_workers; } pub fn can_host_persistent_backend(&self) -> bool { if self.hosting_role != HostingRole::ClientAndServer { return false; } if self.background_execution_limited { return false; } return self.supports_persistent_service; } }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonPolicy { pub product_id: String, pub listen_addr: SocketAddr, pub token_file: PathBuf, pub auth_issuer: String, pub allowed_transitions: BTreeSet<Transition> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedPrincipal { pub subject: String, pub device_id: Option<String>, pub scopes: BTreeSet<String> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateComponent { pub component_id: String, pub from_revision: String, pub to_revision: String, pub expected_digest: Option<String>, pub hot_reloadable: bool }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePlan { pub update_id: String, pub components: Vec<UpdateComponent>, pub drain_timeout_ms: u64 }
impl UpdatePlan { pub fn drain_timeout(&self) -> Duration { return Duration::from_millis(self.drain_timeout_ms); } }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryEvent { pub event_name: String, pub product_id: String, pub generation: Option<u64>, pub update_id: Option<String>, pub update_stage: Option<UpdateStage>, pub deployment_id: Option<String>, pub deployment_stage: Option<DeploymentStage> }
pub trait TelemetrySink: Send + Sync { fn emit(&self, event: TelemetryEvent); }
pub trait ProductDaemonAdapter { fn product_id(&self) -> &str; fn apply_transition(&self, transition: &Transition, desired_state: &DesiredState) -> Result<(), String>; }
pub trait ProductDeploymentAdapter { fn product_id(&self) -> &str; fn plan_deployment(&self, request: &DeployRequest, generation: u64) -> Result<DeploymentPlan, String>; fn activate_deployment(&self, plan: &DeploymentPlan) -> Result<(), String>; }
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError { #[error("daemon must bind to loopback by default")] NonLoopbackBind, #[error("token_file is required")] MissingTokenFile, #[error("auth issuer is required")] MissingAuthIssuer, #[error("transition is not allowed by daemon policy: {0:?}")] TransitionDenied(Transition), #[error("update component must use an exact non-empty revision: {0}")] MutableRevision(String), #[error("principal is missing required scope: {0}")] MissingScope(String), #[error("product identity mismatch: expected {expected}, got {actual}")] ProductIdMismatch { expected: String, actual: String }, #[error("product transition failed: {0}")] ProductTransition(String), #[error("invalid deploy request: {0}")] InvalidDeployRequest(String), #[error("product deployment planning failed: {0}")] DeploymentPlanning(String), #[error("product deployment activation failed: {0}")] DeploymentActivation(String), #[error("ordinary application deployment must not restart the complete desktop appliance")] UnexpectedApplianceRestart, #[error("deployment worker must use an exact revision: {0}")] MutableWorkerRevision(String), #[error("consumer-owned deterministic docs require a pinned api-docs revision")] MissingApiDocsRevision, #[error("deployment docs must use consumer_owned publication mode")] InvalidDocsPublicationMode }
pub fn validate_policy(policy: &DaemonPolicy) -> Result<(), PolicyError> { if !policy.listen_addr.ip().is_loopback() { return Err(PolicyError::NonLoopbackBind); } if policy.token_file.as_os_str().is_empty() { return Err(PolicyError::MissingTokenFile); } if policy.auth_issuer.trim().is_empty() { return Err(PolicyError::MissingAuthIssuer); } return Ok(()); }
pub fn authorize_transition(policy: &DaemonPolicy, transition: Transition) -> Result<(), PolicyError> { if !policy.allowed_transitions.contains(&transition) { return Err(PolicyError::TransitionDenied(transition)); } return Ok(()); }
pub fn authorize_scope(principal: &AuthenticatedPrincipal, required_scope: &str) -> Result<(), PolicyError> { if !principal.scopes.contains(required_scope) { return Err(PolicyError::MissingScope(required_scope.to_string())); } return Ok(()); }
pub fn validate_update_plan(plan: &UpdatePlan) -> Result<(), PolicyError> { for component in &plan.components { if component.to_revision.trim().is_empty() || component.to_revision == "latest" { return Err(PolicyError::MutableRevision(component.component_id.clone())); } } return Ok(()); }
pub fn validate_deployment_plan(request: &DeployRequest, plan: &DeploymentPlan) -> Result<(), PolicyError> { request.validate().map_err(|error| PolicyError::InvalidDeployRequest(error.to_string()))?; if plan.full_appliance_restart_required { return Err(PolicyError::UnexpectedApplianceRestart); } for worker in &plan.workers { if worker.revision.trim().is_empty() || worker.revision == "latest" { return Err(PolicyError::MutableWorkerRevision(worker.service_id.clone())); } } if request.docs_generation == DocsGenerationMode::ConsumerOwned { let docs = plan.docs.as_ref().ok_or(PolicyError::MissingApiDocsRevision)?; if docs.api_docs_revision.trim().is_empty() || matches!(docs.api_docs_revision.as_str(), "latest" | "main" | "master") { return Err(PolicyError::MissingApiDocsRevision); } if docs.publication_mode != "consumer_owned" { return Err(PolicyError::InvalidDocsPublicationMode); } } return Ok(()); }
#[cfg(test)] mod tests { use super::*; use ores_common_desktop_cli::{DeploySource, RouteDiscoveryMode}; #[test] fn ordinary_app_deploy_cannot_restart_entire_appliance() { let request = DeployRequest { source: DeploySource::LocalFolder { path: PathBuf::from("/work/app") }, compose_file: ".ores-compose.yaml".to_string(), route_discovery: RouteDiscoveryMode::ComposeAndBuildMetadata, docs_generation: DocsGenerationMode::Disabled, api_docs_revision: None }; let plan = DeploymentPlan { deployment_id: "deploy-1".to_string(), product_id: "scintilla".to_string(), generation: 3, workers: vec![], route_changes: vec![], docs: None, full_appliance_restart_required: true }; assert_eq!(validate_deployment_plan(&request, &plan), Err(PolicyError::UnexpectedApplianceRestart)); } }
