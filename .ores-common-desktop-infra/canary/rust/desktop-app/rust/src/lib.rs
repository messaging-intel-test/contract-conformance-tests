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

pub fn send_lifecycle_command<A, T>(
    adapter: &A,
    transport: &T,
    request_id: impl Into<String>,
    command: LifecycleCommand,
) -> Result<ControlResponse, DesktopAppError>
where
    A: ProductDesktopAppAdapter,
    T: ControlTransport,
{
    let config = config_from_adapter(adapter)
        .map_err(|error| DesktopAppError::Config(error.to_string()))?;
    let control_request = request(request_id, adapter.product_id(), command);
    let response = transport
        .send(&config, &control_request)
        .map_err(DesktopAppError::Transport)?;

    if !response.accepted {
        return Err(DesktopAppError::Rejected(
            response
                .message
                .clone()
                .unwrap_or_else(|| "daemon rejected request".to_string()),
        ));
    }

    return Ok(response);
}

pub fn send_deploy_command<A, T>(
    adapter: &A,
    transport: &T,
    request_id: impl Into<String>,
    deploy: DeployRequest,
) -> Result<ControlResponse, DesktopAppError>
where
    A: ProductDesktopAppAdapter,
    T: ControlTransport,
{
    let config = config_from_adapter(adapter)
        .map_err(|error| DesktopAppError::Config(error.to_string()))?;
    let control_request = deployment_request(request_id, adapter.product_id(), deploy)
        .map_err(|error| DesktopAppError::InvalidDeploy(error.to_string()))?;
    let response = transport
        .send(&config, &control_request)
        .map_err(DesktopAppError::Transport)?;

    if !response.accepted {
        return Err(DesktopAppError::Rejected(
            response
                .message
                .clone()
                .unwrap_or_else(|| "daemon rejected deployment".to_string()),
        ));
    }

    return Ok(response);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ores_common_desktop_cli::{ControlRequest, DeploySource, ResolvedCliConfig};
    use std::path::PathBuf;

    struct TestApp;

    impl ProductCliAdapter for TestApp {
        fn product_id(&self) -> &str {
            return "beamscale";
        }

        fn resolved_config(&self) -> Result<ResolvedCliConfig, String> {
            return ResolvedCliConfig::loopback(
                "beamscale",
                8765,
                PathBuf::from("/tmp/beamscale.token"),
            )
            .map_err(|error| error.to_string());
        }
    }

    impl ProductDesktopAppAdapter for TestApp {
        fn display_name(&self) -> &str {
            return "BeamScale";
        }
    }

    struct TestTransport;

    impl ControlTransport for TestTransport {
        fn send(
            &self,
            _config: &ResolvedCliConfig,
            request: &ControlRequest,
        ) -> Result<ControlResponse, String> {
            return Ok(ControlResponse {
                request_id: request.request_id.clone(),
                accepted: true,
                generation: Some(4),
                message: None,
            });
        }
    }

    #[test]
    fn product_desktop_app_delegates_to_shared_control_protocol() {
        let response = send_lifecycle_command(
            &TestApp,
            &TestTransport,
            "request-1",
            LifecycleCommand::Status,
        )
        .expect("status should be accepted");

        assert_eq!(response.generation, Some(4));
    }

    #[test]
    fn desktop_app_can_submit_local_folder_deployment() {
        let deploy = DeployRequest::without_docs(DeploySource::LocalFolder {
            path: PathBuf::from("/work/app"),
        });
        let response = send_deploy_command(
            &TestApp,
            &TestTransport,
            "deploy-1",
            deploy,
        )
        .expect("deployment should be accepted");

        assert_eq!(response.generation, Some(4));
    }
}
