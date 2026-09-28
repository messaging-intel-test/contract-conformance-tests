use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use thiserror::Error;

pub const LOCAL_CONTROL_PROTOCOL: &str = "ores.desktop.local-control/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteAuthority {
    Erlang,
    Rust,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteTarget {
    pub host: IpAddr,
    pub port: u16,
}

impl RouteTarget {
    pub fn is_loopback(&self) -> bool {
        return self.host.is_loopback();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSpec {
    pub route_id: String,
    pub host: String,
    pub path_prefix: String,
    pub target: RouteTarget,
    pub public: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSpec {
    pub service_id: String,
    pub revision: String,
    pub digest: Option<String>,
    pub command_id: String,
    pub health_endpoint: Option<RouteTarget>,
    pub hot_reloadable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredState {
    pub product_id: String,
    pub generation: u64,
    pub route_authority: RouteAuthority,
    pub routes: Vec<RouteSpec>,
    pub services: Vec<ServiceSpec>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteChange {
    Add(RouteSpec),
    Replace(RouteSpec),
    Remove { route_id: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("product_id must not be empty")]
    EmptyProductId,
    #[error("duplicate route_id: {0}")]
    DuplicateRoute(String),
    #[error("route target must be loopback unless public exposure is explicitly delegated: {0}")]
    NonLoopbackTarget(String),
    #[error("service revision must be exact and non-empty: {0}")]
    MissingRevision(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error("adapter product_id {adapter_product_id} does not match desired-state product_id {state_product_id}")]
    ProductIdMismatch {
        adapter_product_id: String,
        state_product_id: String,
    },
    #[error("shared desired-state validation failed: {0}")]
    SharedValidation(ValidationError),
    #[error("product desired-state validation failed: {0}")]
    ProductValidation(String),
}

pub trait DesktopInfraAdapter {
    fn product_id(&self) -> &str;
    fn desired_state(&self) -> DesiredState;

    fn validate_product_state(&self, _state: &DesiredState) -> Result<(), String> {
        return Ok(());
    }
}

pub fn validate_desired_state(state: &DesiredState) -> Result<(), ValidationError> {
    if state.product_id.trim().is_empty() {
        return Err(ValidationError::EmptyProductId);
    }

    let mut route_ids = BTreeSet::new();

    for route in &state.routes {
        if !route_ids.insert(route.route_id.clone()) {
            return Err(ValidationError::DuplicateRoute(route.route_id.clone()));
        }

        if !route.public && !route.target.is_loopback() {
            return Err(ValidationError::NonLoopbackTarget(route.route_id.clone()));
        }
    }

    for service in &state.services {
        if service.revision.trim().is_empty() || service.revision == "latest" {
            return Err(ValidationError::MissingRevision(service.service_id.clone()));
        }
    }

    return Ok(());
}

pub fn desired_state_from_adapter<A: DesktopInfraAdapter>(adapter: &A) -> Result<DesiredState, AdapterError> {
    let state = adapter.desired_state();
    let adapter_product_id = adapter.product_id();

    if state.product_id != adapter_product_id {
        return Err(AdapterError::ProductIdMismatch {
            adapter_product_id: adapter_product_id.to_string(),
            state_product_id: state.product_id,
        });
    }

    validate_desired_state(&state).map_err(AdapterError::SharedValidation)?;
    adapter
        .validate_product_state(&state)
        .map_err(AdapterError::ProductValidation)?;

    return Ok(state);
}

pub fn diff_routes(current: &[RouteSpec], desired: &[RouteSpec]) -> Vec<RouteChange> {
    let current_by_id = current
        .iter()
        .map(|route| (route.route_id.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let desired_by_id = desired
        .iter()
        .map(|route| (route.route_id.as_str(), route))
        .collect::<BTreeMap<_, _>>();

    let mut changes = Vec::new();

    for (route_id, route) in &desired_by_id {
        match current_by_id.get(route_id) {
            None => {
                changes.push(RouteChange::Add((**route).clone()));
            }
            Some(existing) if **existing != **route => {
                changes.push(RouteChange::Replace((**route).clone()));
            }
            Some(_) => {}
        }
    }

    for route_id in current_by_id.keys() {
        if !desired_by_id.contains_key(route_id) {
            changes.push(RouteChange::Remove {
                route_id: (*route_id).to_string(),
            });
        }
    }

    return changes;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn route(route_id: &str, port: u16) -> RouteSpec {
        return RouteSpec {
            route_id: route_id.to_string(),
            host: "example.local".to_string(),
            path_prefix: "/".to_string(),
            target: RouteTarget {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
            },
            public: false,
        };
    }

    #[test]
    fn route_diff_is_deterministic() {
        let current = vec![route("a", 8000), route("b", 8001)];
        let desired = vec![route("a", 9000), route("c", 8002)];
        let changes = diff_routes(&current, &desired);

        assert_eq!(changes.len(), 3);
        assert!(matches!(&changes[0], RouteChange::Replace(spec) if spec.route_id == "a"));
        assert!(matches!(&changes[1], RouteChange::Add(spec) if spec.route_id == "c"));
        assert!(matches!(&changes[2], RouteChange::Remove { route_id } if route_id == "b"));
    }

    #[test]
    fn mutable_latest_revision_is_rejected() {
        let state = DesiredState {
            product_id: "scintilla".to_string(),
            generation: 1,
            route_authority: RouteAuthority::Erlang,
            routes: vec![],
            services: vec![ServiceSpec {
                service_id: "ingress".to_string(),
                revision: "latest".to_string(),
                digest: None,
                command_id: "ingress".to_string(),
                health_endpoint: None,
                hot_reloadable: true,
            }],
            labels: BTreeMap::new(),
        };

        assert!(matches!(
            validate_desired_state(&state),
            Err(ValidationError::MissingRevision(service_id)) if service_id == "ingress"
        ));
    }
}
