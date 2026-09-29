use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const CANONICAL_COMPOSE_FILE: &str = ".ores-compose.yaml";
pub const COMPAT_COMPOSE_FILE: &str = "ores-compose.yaml";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleCommand {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationRepositoryPin {
    pub repository: String,
    pub revision: String,
    pub subdir: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DeploySource {
    GitRepository {
        repository: String,
        revision: String,
        subdir: Option<String>,
    },
    GitHubOrganization {
        organization: String,
        repositories: Vec<OrganizationRepositoryPin>,
    },
    LocalFolder {
        path: PathBuf,
    },
}

impl DeploySource {
    pub fn is_remote(&self) -> bool {
        return matches!(self, Self::GitRepository { .. } | Self::GitHubOrganization { .. });
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteDiscoveryMode {
    ComposeAuthority,
    ComposeAndBuildMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsGenerationMode {
    Disabled,
    ConsumerOwned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployRequest {
    pub source: DeploySource,
    pub compose_file: String,
    pub route_discovery: RouteDiscoveryMode,
    pub docs_generation: DocsGenerationMode,
    pub api_docs_revision: Option<String>,
}

impl DeployRequest {
    pub fn canonical(source: DeploySource, api_docs_revision: impl Into<String>) -> Self {
        return Self {
            source,
            compose_file: CANONICAL_COMPOSE_FILE.to_string(),
            route_discovery: RouteDiscoveryMode::ComposeAndBuildMetadata,
            docs_generation: DocsGenerationMode::ConsumerOwned,
            api_docs_revision: Some(api_docs_revision.into()),
        };
    }

    /// Docs-disabled deployment is intentionally limited to local development.
    pub fn without_docs(source: DeploySource) -> Self {
        return Self {
            source,
            compose_file: CANONICAL_COMPOSE_FILE.to_string(),
            route_discovery: RouteDiscoveryMode::ComposeAndBuildMetadata,
            docs_generation: DocsGenerationMode::Disabled,
            api_docs_revision: None,
        };
    }

    pub fn validate(&self) -> Result<(), DeployRequestError> {
        if self.compose_file != CANONICAL_COMPOSE_FILE && self.compose_file != COMPAT_COMPOSE_FILE {
            return Err(DeployRequestError::UnsupportedComposeFile(
                self.compose_file.clone(),
            ));
        }

        match &self.source {
            DeploySource::GitRepository {
                repository,
                revision,
                subdir,
            } => {
                validate_repository(repository)?;
                validate_git_commit(revision)?;
                validate_optional_subdir(subdir)?;
            }
            DeploySource::GitHubOrganization {
                organization,
                repositories,
            } => {
                if organization.trim().is_empty() {
                    return Err(DeployRequestError::EmptyOrganization);
                }

                if repositories.is_empty() {
                    return Err(DeployRequestError::EmptyOrganizationPlan);
                }

                let mut seen_repositories = BTreeSet::new();

                for repository in repositories {
                    validate_organization_repository_name(&repository.repository)?;
                    validate_git_commit(&repository.revision)?;
                    validate_optional_subdir(&repository.subdir)?;

                    if !seen_repositories.insert(repository.repository.to_ascii_lowercase()) {
                        return Err(DeployRequestError::DuplicateOrganizationRepository(
                            repository.repository.clone(),
                        ));
                    }
                }
            }
            DeploySource::LocalFolder { path } => {
                if path.as_os_str().is_empty() {
                    return Err(DeployRequestError::EmptyLocalFolder);
                }
            }
        }

        match self.docs_generation {
            DocsGenerationMode::ConsumerOwned => {
                let api_docs_revision = self
                    .api_docs_revision
                    .as_deref()
                    .ok_or(DeployRequestError::MissingApiDocsRevision)?;
                validate_api_docs_revision(api_docs_revision)?;
            }
            DocsGenerationMode::Disabled => {
                if self.source.is_remote() {
                    return Err(DeployRequestError::DocsRequiredForRemoteSource);
                }

                if self.api_docs_revision.is_some() {
                    return Err(DeployRequestError::UnexpectedApiDocsRevision);
                }
            }
        }

        return Ok(());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ControlPayload {
    Deploy(DeployRequest),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequest {
    pub request_id: String,
    pub product_id: String,
    pub command: LifecycleCommand,
    pub generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ControlPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlResponse {
    pub request_id: String,
    pub accepted: bool,
    pub generation: Option<u64>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCliConfig {
    pub product_id: String,
    pub daemon_endpoint: SocketAddr,
    pub token_file: PathBuf,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeployRequestError {
    #[error("repository must not be empty")]
    EmptyRepository,
    #[error("organization must not be empty")]
    EmptyOrganization,
    #[error("organization deployment must contain an explicit repository plan")]
    EmptyOrganizationPlan,
    #[error("organization repository name is invalid: {0}")]
    InvalidOrganizationRepository(String),
    #[error("organization deployment contains duplicate repository: {0}")]
    DuplicateOrganizationRepository(String),
    #[error("local folder must not be empty")]
    EmptyLocalFolder,
    #[error("deployment subdir must be a safe relative path: {0}")]
    InvalidSubdir(String),
    #[error("unsupported ores-compose manifest name: {0}")]
    UnsupportedComposeFile(String),
    #[error("remote deployment source must pin a full immutable git commit SHA: {0}")]
    MutableRevision(String),
    #[error("consumer-owned deterministic docs require a pinned api-docs revision")]
    MissingApiDocsRevision,
    #[error("api-docs generation must pin a full immutable git commit SHA: {0}")]
    MutableApiDocsRevision(String),
    #[error("remote deployment sources require deterministic consumer-owned api-docs")]
    DocsRequiredForRemoteSource,
    #[error("api_docs_revision must be absent when docs generation is disabled")]
    UnexpectedApiDocsRevision,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliConfigError {
    #[error("product_id must not be empty")]
    EmptyProductId,
    #[error("daemon endpoint must be loopback")]
    NonLoopbackDaemon,
    #[error("daemon endpoint port must not be zero")]
    InvalidDaemonPort,
    #[error("token_file must be a filesystem path supplied by resolved runtime configuration")]
    MissingTokenFile,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliAdapterError {
    #[error("product CLI adapter failed to resolve configuration: {0}")]
    ProductConfig(String),
    #[error("shared CLI validation failed: {0}")]
    SharedConfig(CliConfigError),
    #[error("CLI product identity mismatch: expected {expected}, got {actual}")]
    ProductIdMismatch { expected: String, actual: String },
}

pub trait ProductCliAdapter {
    fn product_id(&self) -> &str;
    fn resolved_config(&self) -> Result<ResolvedCliConfig, String>;
}

pub trait ControlTransport {
    fn send(&self, config: &ResolvedCliConfig, request: &ControlRequest) -> Result<ControlResponse, String>;
}

pub fn is_full_git_commit_sha(value: &str) -> bool {
    let value = value.trim();
    let is_supported_length = value.len() == 40 || value.len() == 64;

    return is_supported_length && value.as_bytes().iter().all(u8::is_ascii_hexdigit);
}

fn validate_repository(repository: &str) -> Result<(), DeployRequestError> {
    if repository.trim().is_empty() {
        return Err(DeployRequestError::EmptyRepository);
    }

    return Ok(());
}

fn validate_organization_repository_name(repository: &str) -> Result<(), DeployRequestError> {
    let repository = repository.trim();

    if repository.is_empty()
        || repository == "."
        || repository == ".."
        || repository.contains('/')
        || repository.contains('\\')
    {
        return Err(DeployRequestError::InvalidOrganizationRepository(
            repository.to_string(),
        ));
    }

    return Ok(());
}

fn validate_git_commit(revision: &str) -> Result<(), DeployRequestError> {
    if !is_full_git_commit_sha(revision) {
        return Err(DeployRequestError::MutableRevision(revision.to_string()));
    }

    return Ok(());
}

fn validate_api_docs_revision(revision: &str) -> Result<(), DeployRequestError> {
    if !is_full_git_commit_sha(revision) {
        return Err(DeployRequestError::MutableApiDocsRevision(
            revision.to_string(),
        ));
    }

    return Ok(());
}

fn validate_optional_subdir(subdir: &Option<String>) -> Result<(), DeployRequestError> {
    let Some(subdir) = subdir else {
        return Ok(());
    };
    let path = Path::new(subdir);

    if subdir.trim().is_empty() || path.is_absolute() {
        return Err(DeployRequestError::InvalidSubdir(subdir.clone()));
    }

    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err(DeployRequestError::InvalidSubdir(subdir.clone()));
        }
    }

    return Ok(());
}

impl ResolvedCliConfig {
    pub fn loopback(product_id: impl Into<String>, port: u16, token_file: PathBuf) -> Result<Self, CliConfigError> {
        let config = Self {
            product_id: product_id.into(),
            daemon_endpoint: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            token_file,
        };

        return config.validate().map(|()| config);
    }

    pub fn validate(&self) -> Result<(), CliConfigError> {
        if self.product_id.trim().is_empty() {
            return Err(CliConfigError::EmptyProductId);
        }

        if !self.daemon_endpoint.ip().is_loopback() {
            return Err(CliConfigError::NonLoopbackDaemon);
        }

        if self.daemon_endpoint.port() == 0 {
            return Err(CliConfigError::InvalidDaemonPort);
        }

        if self.token_file.as_os_str().is_empty() {
            return Err(CliConfigError::MissingTokenFile);
        }

        return Ok(());
    }
}

pub fn config_from_adapter<A: ProductCliAdapter>(adapter: &A) -> Result<ResolvedCliConfig, CliAdapterError> {
    let config = adapter
        .resolved_config()
        .map_err(CliAdapterError::ProductConfig)?;

    config.validate().map_err(CliAdapterError::SharedConfig)?;

    if config.product_id != adapter.product_id() {
        return Err(CliAdapterError::ProductIdMismatch {
            expected: adapter.product_id().to_string(),
            actual: config.product_id,
        });
    }

    return Ok(config);
}

pub fn request(request_id: impl Into<String>, product_id: impl Into<String>, command: LifecycleCommand) -> ControlRequest {
    return ControlRequest {
        request_id: request_id.into(),
        product_id: product_id.into(),
        command,
        generation: None,
        payload: None,
    };
}

pub fn deployment_request(
    request_id: impl Into<String>,
    product_id: impl Into<String>,
    deploy: DeployRequest,
) -> Result<ControlRequest, DeployRequestError> {
    deploy.validate()?;

    return Ok(ControlRequest {
        request_id: request_id.into(),
        product_id: product_id.into(),
        command: LifecycleCommand::Deploy,
        generation: None,
        payload: Some(ControlPayload::Deploy(deploy)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const API_DOCS_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn loopback_constructor_is_safe_by_default() {
        let config = ResolvedCliConfig::loopback(
            "beamscale",
            8765,
            PathBuf::from("/tmp/desktop-daemon.token"),
        )
        .expect("valid loopback config");

        assert!(config.daemon_endpoint.ip().is_loopback());
    }

    #[test]
    fn deployment_can_point_at_repo_with_compose_authority() {
        let deploy = DeployRequest::canonical(
            DeploySource::GitRepository {
                repository: "https://github.com/example/app".to_string(),
                revision: COMMIT_SHA.to_string(),
                subdir: None,
            },
            API_DOCS_SHA,
        );
        let request = deployment_request("req-1", "scintilla", deploy)
            .expect("immutable deployment source should validate");

        assert_eq!(request.command, LifecycleCommand::Deploy);
        assert!(matches!(request.payload, Some(ControlPayload::Deploy(_))));
    }

    #[test]
    fn branch_name_deployment_is_rejected() {
        let deploy = DeployRequest::canonical(
            DeploySource::GitRepository {
                repository: "https://github.com/example/app".to_string(),
                revision: "develop".to_string(),
                subdir: None,
            },
            API_DOCS_SHA,
        );

        assert!(matches!(
            deploy.validate(),
            Err(DeployRequestError::MutableRevision(revision)) if revision == "develop"
        ));
    }

    #[test]
    fn organization_deploy_requires_explicit_pinned_repositories() {
        let deploy = DeployRequest::canonical(
            DeploySource::GitHubOrganization {
                organization: "example".to_string(),
                repositories: vec![],
            },
            API_DOCS_SHA,
        );

        assert_eq!(
            deploy.validate(),
            Err(DeployRequestError::EmptyOrganizationPlan)
        );
    }

    #[test]
    fn remote_deploy_cannot_disable_deterministic_docs() {
        let deploy = DeployRequest::without_docs(DeploySource::GitRepository {
            repository: "https://github.com/example/app".to_string(),
            revision: COMMIT_SHA.to_string(),
            subdir: None,
        });

        assert_eq!(
            deploy.validate(),
            Err(DeployRequestError::DocsRequiredForRemoteSource)
        );
    }

    #[test]
    fn parent_directory_subdir_is_rejected() {
        let deploy = DeployRequest::canonical(
            DeploySource::GitRepository {
                repository: "https://github.com/example/app".to_string(),
                revision: COMMIT_SHA.to_string(),
                subdir: Some("../secret".to_string()),
            },
            API_DOCS_SHA,
        );

        assert!(matches!(
            deploy.validate(),
            Err(DeployRequestError::InvalidSubdir(subdir)) if subdir == "../secret"
        ));
    }

    struct TestCliAdapter;

    impl ProductCliAdapter for TestCliAdapter {
        fn product_id(&self) -> &str {
            return "scintilla";
        }

        fn resolved_config(&self) -> Result<ResolvedCliConfig, String> {
            return ResolvedCliConfig::loopback(
                "scintilla",
                8765,
                PathBuf::from("/tmp/scintilla.token"),
            )
            .map_err(|error| error.to_string());
        }
    }

    #[test]
    fn thin_cli_entrypoint_can_delegate_resolved_config() {
        let config = config_from_adapter(&TestCliAdapter).expect("adapter config should validate");

        assert_eq!(config.product_id, "scintilla");
        assert!(config.daemon_endpoint.ip().is_loopback());
    }
}
