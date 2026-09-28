use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
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
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DeploySource {
    GitRepository {
        repository: String,
        revision: Option<String>,
        subdir: Option<String>,
    },
    GitHubOrganization {
        organization: String,
        repository_filter: Option<String>,
        revision: Option<String>,
    },
    LocalFolder {
        path: PathBuf,
    },
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
    pub fn canonical(source: DeploySource) -> Self {
        return Self {
            source,
            compose_file: CANONICAL_COMPOSE_FILE.to_string(),
            route_discovery: RouteDiscoveryMode::ComposeAndBuildMetadata,
            docs_generation: DocsGenerationMode::ConsumerOwned,
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
                ..
            } => {
                if repository.trim().is_empty() {
                    return Err(DeployRequestError::EmptyRepository);
                }

                if matches!(revision.as_deref(), Some("latest" | "main" | "master")) {
                    return Err(DeployRequestError::MutableRevision(
                        revision.clone().unwrap_or_default(),
                    ));
                }
            }
            DeploySource::GitHubOrganization {
                organization,
                revision,
                ..
            } => {
                if organization.trim().is_empty() {
                    return Err(DeployRequestError::EmptyOrganization);
                }

                if matches!(revision.as_deref(), Some("latest")) {
                    return Err(DeployRequestError::MutableRevision(
                        revision.clone().unwrap_or_default(),
                    ));
                }
            }
            DeploySource::LocalFolder { path } => {
                if path.as_os_str().is_empty() {
                    return Err(DeployRequestError::EmptyLocalFolder);
                }
            }
        }

        if self.docs_generation == DocsGenerationMode::ConsumerOwned {
            if matches!(self.api_docs_revision.as_deref(), Some("latest" | "main" | "master")) {
                return Err(DeployRequestError::MutableApiDocsRevision(
                    self.api_docs_revision.clone().unwrap_or_default(),
                ));
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
    #[error("local folder must not be empty")]
    EmptyLocalFolder,
    #[error("unsupported ores-compose manifest name: {0}")]
    UnsupportedComposeFile(String),
    #[error("deployment source must use an immutable revision when one is supplied: {0}")]
    MutableRevision(String),
    #[error("api-docs generation must pin an immutable api-docs revision: {0}")]
    MutableApiDocsRevision(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliConfigError {
    #[error("product_id must not be empty")]
    EmptyProductId,
    #[error("daemon endpoint must be loopback")]
    NonLoopbackDaemon,
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

    #[test]
    fn deployment_can_point_at_repo_with_compose_authority() {
        let deploy = DeployRequest::canonical(DeploySource::GitRepository {
            repository: "https://github.com/example/app".to_string(),
            revision: Some("0123456789abcdef".to_string()),
            subdir: None,
        });
        let request = deployment_request("req-1", "scintilla", deploy)
            .expect("immutable deployment source should validate");

        assert_eq!(request.command, LifecycleCommand::Deploy);
        assert!(matches!(request.payload, Some(ControlPayload::Deploy(_))));
    }

    #[test]
    fn mutable_latest_deployment_is_rejected() {
        let deploy = DeployRequest::canonical(DeploySource::GitRepository {
            repository: "https://github.com/example/app".to_string(),
            revision: Some("latest".to_string()),
            subdir: None,
        });

        assert!(matches!(
            deploy.validate(),
            Err(DeployRequestError::MutableRevision(revision)) if revision == "latest"
        ));
    }
}
