variable "product_id" {
  type        = string
  description = "Stable product identifier used by the local appliance."

  validation {
    condition     = length(trimspace(var.product_id)) > 0
    error_message = "product_id must not be empty."
  }
}

variable "data_dir" {
  type        = string
  description = "Product-specific local state directory."
}

variable "daemon_port" {
  type        = number
  description = "Authenticated loopback daemon port."
}

variable "ingress_port" {
  type        = number
  description = "Stable local ingress port."
}

variable "route_authority" {
  type        = string
  default     = "erlang"
  description = "Dynamic route-table authority. Erlang is preferred for hot loading."

  validation {
    condition     = contains(["erlang", "rust"], var.route_authority)
    error_message = "route_authority must be erlang or rust."
  }
}

variable "front_proxy" {
  type        = string
  default     = "none"
  description = "Optional socket/TLS forwarding layer. It is never the dynamic route authority."

  validation {
    condition     = contains(["none", "nginx", "haproxy"], var.front_proxy)
    error_message = "front_proxy must be none, nginx, or haproxy."
  }
}

variable "auth_issuer" {
  type        = string
  default     = "https://ores-shared-auth.com"
  description = "Shared-auth issuer/service identity."
}

variable "otel_service_name" {
  type        = string
  description = "OpenTelemetry service.name value used by the product adapter."
}

variable "component_revisions" {
  type        = map(string)
  default     = {}
  description = "Exact component revisions. Mutable 'latest' is not a release contract."
}
