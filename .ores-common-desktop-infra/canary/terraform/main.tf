locals {
  local_appliance = {
    protocol            = "ores.desktop.local-control/v1"
    product_id          = var.product_id
    data_dir            = var.data_dir
    daemon              = "127.0.0.1:${var.daemon_port}"
    ingress             = "127.0.0.1:${var.ingress_port}"
    route_authority     = var.route_authority
    front_proxy         = var.front_proxy
    auth_issuer         = var.auth_issuer
    otel_service_name   = var.otel_service_name
    component_revisions = var.component_revisions
  }

  mutable_revisions = [
    for component, revision in var.component_revisions : component
    if trimspace(revision) == "" || revision == "latest"
  ]
}

check "exact_revisions" {
  assert {
    condition     = length(local.mutable_revisions) == 0
    error_message = "component_revisions must be exact and may not use latest."
  }
}
