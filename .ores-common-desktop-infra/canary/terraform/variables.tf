variable "product_id" {
  type = string
  validation { condition = length(trimspace(var.product_id)) > 0; error_message = "product_id must not be empty." }
}
variable "data_dir" { type = string }
variable "daemon_port" { type = number }
variable "ingress_port" { type = number }
variable "route_authority" { type = string; default = "erlang"; validation { condition = contains(["erlang", "rust"], var.route_authority); error_message = "route_authority must be erlang or rust." } }
variable "front_proxy" { type = string; default = "none"; validation { condition = contains(["none", "nginx", "haproxy"], var.front_proxy); error_message = "front_proxy must be none, nginx, or haproxy." } }
variable "auth_issuer" { type = string; default = "https://ores-shared-auth.com" }
variable "otel_service_name" { type = string }
variable "component_revisions" { type = map(string); default = {} }
