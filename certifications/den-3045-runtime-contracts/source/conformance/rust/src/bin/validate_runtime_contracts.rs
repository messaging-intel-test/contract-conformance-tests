use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
};

const SCHEMA: &str = "ores.desktop-runtime-contracts/v1";
const PRODUCTS_SCHEMA: &str = "ores.desktop-products/v1";
const MAX_CLIENT_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("desktop runtime contract validation failed: {error}");
            2
        }
    };

    std::process::exit(code);
}

fn run() -> Result<(), String> {
    let contract_path = env::var("ORES_DESKTOP_RUNTIME_CONTRACTS_PATH")
        .unwrap_or_else(|_| "fleet/runtime-contracts.json".to_owned());
    let products_path = env::var("ORES_DESKTOP_PRODUCTS_PATH")
        .unwrap_or_else(|_| "fleet/products.json".to_owned());

    let contracts = fs::read_to_string(&contract_path)
        .map_err(|error| format!("cannot read runtime contracts {contract_path:?}: {error}"))?;
    let products = fs::read_to_string(&products_path)
        .map_err(|error| format!("cannot read product inventory {products_path:?}: {error}"))?;

    validate_runtime_contracts_json(&contracts, &products)?;
    println!("desktop runtime contracts OK: {contract_path}");
    return Ok(());
}

fn validate_runtime_contracts_json(contracts_input: &str, products_input: &str) -> Result<(), String> {
    let contracts: Value = serde_json::from_str(contracts_input)
        .map_err(|error| format!("runtime contracts are not valid JSON: {error}"))?;
    let products: Value = serde_json::from_str(products_input)
        .map_err(|error| format!("product inventory is not valid JSON: {error}"))?;

    require_object_keys(&contracts, "runtime-contracts root", &["schema", "contracts"])?;
    require_object_keys(&products, "products root", &["schema", "products"])?;
    require_string(&contracts, "/schema", SCHEMA)?;
    require_string(&products, "/schema", PRODUCTS_SCHEMA)?;

    let entries = contracts
        .get("contracts")
        .and_then(Value::as_array)
        .ok_or_else(|| "contracts must be an array".to_owned())?;
    if entries.is_empty() {
        return Err("contracts must not be empty".to_owned());
    }

    let inventory = products
        .get("products")
        .and_then(Value::as_array)
        .ok_or_else(|| "products must be an array".to_owned())?;
    if inventory.is_empty() {
        return Err("product inventory must not be empty".to_owned());
    }

    let mut inventory_orgs = BTreeMap::<String, String>::new();
    for product in inventory {
        let product_id = required_text(product, "product_id")?;
        let org = required_text(product, "org")?;
        if inventory_orgs
            .insert(product_id.to_owned(), org.to_owned())
            .is_some()
        {
            return Err(format!("duplicate inventory product_id: {product_id}"));
        }
    }

    let mut contract_ids = BTreeSet::new();
    let mut ports = BTreeMap::<u64, String>::new();
    let mut delegations = Vec::<(String, String)>::new();

    for entry in entries {
        require_object_keys(
            entry,
            "runtime contract",
            &[
                "product_id",
                "org",
                "daemon_port",
                "client_security",
                "execution",
            ],
        )?;
        let product_id = required_text(entry, "product_id")?;
        let org = required_text(entry, "org")?;
        if !contract_ids.insert(product_id.to_owned()) {
            return Err(format!("duplicate runtime contract product_id: {product_id}"));
        }

        let inventory_org = inventory_orgs
            .get(product_id)
            .ok_or_else(|| format!("runtime contract {product_id} is not present in product inventory"))?;
        if inventory_org != org {
            return Err(format!(
                "{product_id}: runtime contract org {org:?} does not match inventory org {inventory_org:?}"
            ));
        }

        let port = entry
            .get("daemon_port")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{product_id}: daemon_port must be an integer"))?;
        if port == 0 || port > u16::MAX as u64 {
            return Err(format!("{product_id}: daemon_port is outside 1..=65535"));
        }
        if let Some(other) = ports.insert(port, product_id.to_owned()) {
            return Err(format!(
                "daemon port collision on {port}: {other} and {product_id}"
            ));
        }

        validate_client_security(product_id, entry)?;
        validate_execution(product_id, entry, &mut delegations)?;
    }

    let inventory_ids = inventory_orgs.keys().cloned().collect::<BTreeSet<_>>();
    if contract_ids != inventory_ids {
        let missing = inventory_ids
            .difference(&contract_ids)
            .cloned()
            .collect::<Vec<_>>();
        let phantom = contract_ids
            .difference(&inventory_ids)
            .cloned()
            .collect::<Vec<_>>();
        return Err(format!(
            "runtime contracts and product inventory must contain the same products; missing={missing:?} phantom={phantom:?}"
        ));
    }

    if !contract_ids.contains("indiebuild") {
        return Err("delegated IndieBuild product contract is required".to_owned());
    }

    for (source, target) in delegations {
        if source == target {
            return Err(format!("{source}: product cannot delegate to itself"));
        }
        if !contract_ids.contains(&target) {
            return Err(format!("{source}: delegation target {target} is not declared"));
        }
    }

    return Ok(());
}

fn validate_client_security(product_id: &str, entry: &Value) -> Result<(), String> {
    let security = entry
        .get("client_security")
        .ok_or_else(|| format!("{product_id}: client_security is required"))?;
    require_object_keys(
        security,
        &format!("{product_id}.client_security"),
        &[
            "local_bearer_only",
            "literal_loopback_http",
            "explicit_port",
            "redirects_allowed",
            "max_response_bytes",
            "token_file_regular_non_symlink",
            "token_file_private_on_unix",
        ],
    )?;

    for field in [
        "local_bearer_only",
        "literal_loopback_http",
        "explicit_port",
        "token_file_regular_non_symlink",
        "token_file_private_on_unix",
    ] {
        if security.get(field).and_then(Value::as_bool) != Some(true) {
            return Err(format!("{product_id}: client_security.{field} must be true"));
        }
    }

    if security.get("redirects_allowed").and_then(Value::as_bool) != Some(false) {
        return Err(format!(
            "{product_id}: client_security.redirects_allowed must be false"
        ));
    }

    let max_response_bytes = security
        .get("max_response_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{product_id}: max_response_bytes must be an integer"))?;
    if max_response_bytes == 0 || max_response_bytes > MAX_CLIENT_RESPONSE_BYTES {
        return Err(format!(
            "{product_id}: max_response_bytes must be within 1..={MAX_CLIENT_RESPONSE_BYTES}"
        ));
    }

    return Ok(());
}

fn validate_execution(
    product_id: &str,
    entry: &Value,
    delegations: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let execution = entry
        .get("execution")
        .ok_or_else(|| format!("{product_id}: execution is required"))?;
    require_object_keys(
        execution,
        &format!("{product_id}.execution"),
        &[
            "reuse_model",
            "reuse_key",
            "runtime_host_reuse_allowed",
            "direct_worker_launch_from_client",
            "delegates_to",
        ],
    )?;
    let reuse_model = required_text(execution, "reuse_model")?;
    let reuse_key = string_array(execution, "reuse_key")?;

    if execution
        .get("direct_worker_launch_from_client")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err(format!(
            "{product_id}: direct_worker_launch_from_client must be false"
        ));
    }

    let host_reuse = execution
        .get("runtime_host_reuse_allowed")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{product_id}: runtime_host_reuse_allowed must be boolean"))?;
    let delegates_to = execution.get("delegates_to");

    match reuse_model {
        "tenant_generation_cell" => {
            let expected = ["tenant_id", "deployment_generation"];
            if reuse_key.as_slice() != expected {
                return Err(format!(
                    "{product_id}: tenant_generation_cell reuse_key must be tenant_id + deployment_generation"
                ));
            }
            if !host_reuse {
                return Err(format!(
                    "{product_id}: tenant_generation_cell requires runtime host reuse"
                ));
            }
            require_null(delegates_to, product_id)?;
        }
        "fresh_process" => {
            require_empty_key(product_id, &reuse_key)?;
            if host_reuse {
                return Err(format!(
                    "{product_id}: fresh_process cannot advertise runtime host reuse"
                ));
            }
            require_null(delegates_to, product_id)?;
        }
        "fresh_actor" | "fresh_store" | "runtime_managed" => {
            require_empty_key(product_id, &reuse_key)?;
            require_null(delegates_to, product_id)?;
        }
        "delegated" => {
            require_empty_key(product_id, &reuse_key)?;
            if host_reuse {
                return Err(format!(
                    "{product_id}: delegated product layer cannot claim runtime host reuse"
                ));
            }
            let target = delegates_to
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("{product_id}: delegated execution requires delegates_to"))?;
            delegations.push((product_id.to_owned(), target.to_owned()));
        }
        other => {
            return Err(format!("{product_id}: unsupported reuse_model {other:?}"));
        }
    }

    if product_id == "indiebuild"
        && (reuse_model != "delegated"
            || execution.get("delegates_to").and_then(Value::as_str) != Some("scintilla"))
    {
        return Err("indiebuild must delegate execution to scintilla".to_owned());
    }

    if product_id == "pony-expres" && reuse_model != "fresh_process" {
        return Err("pony-expres must preserve fresh-process/no-reuse semantics".to_owned());
    }
    if product_id == "lunatic-lorry" && reuse_model != "fresh_actor" {
        return Err("lunatic-lorry must preserve fresh actor semantics".to_owned());
    }
    if product_id == "wasm-xprs" && reuse_model != "fresh_store" {
        return Err("wasm-xprs must preserve fresh Wasmtime Store semantics".to_owned());
    }
    if matches!(product_id, "iso-lattes" | "graal-show")
        && reuse_model != "tenant_generation_cell"
    {
        return Err(format!(
            "{product_id} must pin reusable cells to tenant + deployment generation"
        ));
    }

    return Ok(());
}

fn require_object_keys(value: &Value, context: &str, allowed: &[&str]) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    for key in object.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(format!("{context} contains unknown field {key:?}"));
        }
    }
    return Ok(());
}

fn require_string(value: &Value, pointer: &str, expected: &str) -> Result<(), String> {
    if value.pointer(pointer).and_then(Value::as_str) != Some(expected) {
        return Err(format!("{pointer} must equal {expected:?}"));
    }
    return Ok(());
}

fn required_text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    return value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("{field} must be a non-empty string"));
}

fn string_array<'a>(value: &'a Value, field: &str) -> Result<Vec<&'a str>, String> {
    let values = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{field} must be an array"))?;
    let mut output = Vec::with_capacity(values.len());
    for item in values {
        let text = item
            .as_str()
            .ok_or_else(|| format!("{field} entries must be strings"))?;
        output.push(text);
    }
    return Ok(output);
}

fn require_empty_key(product_id: &str, reuse_key: &[&str]) -> Result<(), String> {
    if !reuse_key.is_empty() {
        return Err(format!(
            "{product_id}: reuse_key must be empty for this reuse model"
        ));
    }
    return Ok(());
}

fn require_null(value: Option<&Value>, product_id: &str) -> Result<(), String> {
    if value.is_some_and(|value| !value.is_null()) {
        return Err(format!("{product_id}: delegates_to must be null"));
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTRACTS: &str = include_str!("../../../../fleet/runtime-contracts.json");
    const PRODUCTS: &str = include_str!("../../../../fleet/products.json");

    #[test]
    fn canonical_fleet_contract_is_valid() {
        assert_eq!(validate_runtime_contracts_json(CONTRACTS, PRODUCTS), Ok(()));
    }

    #[test]
    fn duplicate_daemon_ports_fail_closed() {
        let mutated = CONTRACTS.replacen("\"daemon_port\": 8766", "\"daemon_port\": 8765", 1);
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("duplicate ports must fail");
        assert!(error.contains("daemon port collision"));
    }

    #[test]
    fn redirectable_local_bearer_client_fails_closed() {
        let mutated = CONTRACTS.replacen(
            "\"redirects_allowed\": false",
            "\"redirects_allowed\": true",
            1,
        );
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("redirects must fail");
        assert!(error.contains("redirects_allowed"));
    }

    #[test]
    fn tenant_generation_reuse_cannot_drop_generation_identity() {
        let mutated = CONTRACTS.replacen(
            "[\"tenant_id\", \"deployment_generation\"]",
            "[\"tenant_id\"]",
            1,
        );
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("reuse-key drift must fail");
        assert!(error.contains("tenant_id + deployment_generation"));
    }

    #[test]
    fn indiebuild_cannot_bypass_scintilla() {
        let mutated = CONTRACTS.replacen(
            "\"delegates_to\": \"scintilla\"",
            "\"delegates_to\": \"pony-expres\"",
            1,
        );
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("IndieBuild delegation drift must fail");
        assert!(error.contains("indiebuild must delegate execution to scintilla"));
    }

    #[test]
    fn unknown_security_field_fails_closed() {
        let mutated = CONTRACTS.replacen(
            "\"local_bearer_only\": true,",
            "\"local_bearer_only\": true, \"local_bearer_onyl\": true,",
            1,
        );
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("unknown security fields must fail");
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn phantom_runtime_contract_fails_closed() {
        let mut contracts: Value = serde_json::from_str(CONTRACTS).expect("canonical contracts");
        let first = contracts["contracts"][0].clone();
        let mut phantom = first;
        phantom["product_id"] = Value::String("phantom-product".to_owned());
        phantom["org"] = Value::String("phantom-org".to_owned());
        phantom["daemon_port"] = Value::from(65534_u64);
        contracts["contracts"]
            .as_array_mut()
            .expect("contracts array")
            .push(phantom);
        let serialized = serde_json::to_string(&contracts).expect("serialize contracts");
        let error = validate_runtime_contracts_json(&serialized, PRODUCTS)
            .expect_err("phantom contracts must fail");
        assert!(error.contains("not present in product inventory"));
    }

    #[test]
    fn inventory_org_drift_fails_closed() {
        let mutated = CONTRACTS.replacen(
            "\"org\": \"iso-lattes\"",
            "\"org\": \"wrong-org\"",
            1,
        );
        let error = validate_runtime_contracts_json(&mutated, PRODUCTS)
            .expect_err("org drift must fail");
        assert!(error.contains("does not match inventory org"));
    }
}
