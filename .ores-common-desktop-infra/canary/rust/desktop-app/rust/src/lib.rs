use ores_common_desktop_cli::{
    config_from_adapter, deployment_request, request, ControlResponse, ControlTransport,
    DeployRequest, LifecycleCommand, ProductCliAdapter,
};
use ores_common_desktop_daemon::{DeploymentStage, DeviceCapabilities, UpdateStage};
use ores_common_desktop_infra::{DesiredState, RouteSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopAppViewState {
    pub product_id: String,
    pub connected: bool,
    pub generation: Option<u64>,
    pub routes: Vec<RouteSpec>,
    pub device_capabilities: Option<DeviceCapabilities>,
    pub update_stage: Option<UpdateStage>,
    pub deployment_stage: Option<DeploymentStage>,
    pub status_message: Option<String>,
}

impl DesktopAppViewState {
    pub fn disconnected(product_id: impl Into<String>) -> Self {
        return Self {
            product_id: product_id.into(),
            connected: false,
            generation: None,
            routes: vec![],
            device_capabilities: None,
            update_stage: None,
            deployment_stage: None,
            status_message: None,
        };
    }

    pub fn apply_desired_state(mut self, desired_state: &DesiredState) -> Self {
        self.generation = Some(desired_state.generation);
        self.routes = desired_state.routes.clone();
        return self;
    }
}

pub trait ProductDesktopAppAdapter: ProductCliAdapter {
    fn display_name(&self) -> &str;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DesktopAppError {
    #[error("desktop app configuration failed: {0}")]
    Config(String),
    #[error("desktop app deploy request is invalid: {0}")]
    InvalidDeploy(String),
    #[error("desktop app daemon request failed: {0}")]
    Transport(String),
    #[error("daemon rejected request: {0}")]
    Rejected(String),
}

pub fn send_lifecycle_command<A, T>(adapter: &A, transport: &T, request_id: impl Into<String>, command: LifecycleCommand) -> Result<ControlResponse, DesktopAppError>
where A: ProductDesktopAppAdapter, T: ControlTransport {
    let config = config_from_adapter(adapter).map_err(|error| DesktopAppError::Config(error.to_string()))?;
    let control_request = request(request_id, adapter.product_id(), command);
    let response = transport.send(&config, &control_request).map_err(DesktopAppError::Transport)?;
    if !response.accepted { return Err(DesktopAppError::Rejected(response.message.clone().unwrap_or_else(|| "daemon rejected request".to_string()))); }
    return Ok(response);
}

pub fn send_deploy_command<A, T>(adapter: &A, transport: &T, request_id: impl Into<String>, deploy: DeployRequest) -> Result<ControlResponse, DesktopAppError>
where A: ProductDesktopAppAdapter, T: ControlTransport {
    let config = config_from_adapter(adapter).map_err(|error| DesktopAppError::Config(error.to_string()))?;
    let control_request = deployment_request(request_id, adapter.product_id(), deploy).map_err(|error| DesktopAppError::InvalidDeploy(error.to_string()))?;
    let response = transport.send(&config, &control_request).map_err(DesktopAppError::Transport)?;
    if !response.accepted { return Err(DesktopAppError::Rejected(response.message.clone().unwrap_or_else(|| "daemon rejected deployment".to_string()))); }
    return Ok(response);
}
