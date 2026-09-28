mod daemon_client;

use anyhow::Result;
use daemon_client::DaemonClient;
use serde_json::Value;
use slint::ComponentHandle;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

slint::slint! {
    import { Button, CheckBox, LineEdit } from "std-widgets.slint";

    export component MainWindow inherits Window {
        title: "BeamScale";
        width: 960px;
        height: 680px;

        in-out property <string> status_text: "BeamScale desktop daemon not checked yet.";
        in-out property <bool> keep_awake: false;

        callback refresh();
        callback doctor();
        callback runtime_start(string);
        callback runtime_stop();
        callback runtime_restart();
        callback tunnel_start(string, string);
        callback tunnel_stop();
        callback tunnel_restart();
        callback keep_awake_changed(bool);
        callback update_status();
        callback update_apply();

        VerticalLayout {
            padding: 24px;
            spacing: 12px;

            Text { text: "BeamScale Local"; font-size: 28px; }
            Text {
                text: "One local BEAM VM, Erlang actor workers, and a Cloudflare Tunnel managed by the desktop daemon.";
                wrap: word-wrap;
            }

            Rectangle { height: 1px; background: #d0d0d0; }

            Text {
                text: root.status_text;
                wrap: word-wrap;
            }

            HorizontalLayout {
                spacing: 8px;
                Button {
                    text: "Refresh status";
                    clicked => { root.refresh(); }
                }
                Button {
                    text: "Doctor";
                    clicked => { root.doctor(); }
                }
                Button {
                    text: "Check updates";
                    clicked => { root.update_status(); }
                }
                Button {
                    text: "Apply updates";
                    clicked => { root.update_apply(); }
                }
            }

            Rectangle { height: 1px; background: #d0d0d0; }

            Text { text: "Local BeamScale runtime"; font-size: 20px; }
            project_input := LineEdit {
                text: ".";
                placeholder-text: "Project directory";
            }
            HorizontalLayout {
                spacing: 8px;
                Button {
                    text: "Start";
                    clicked => { root.runtime_start(project_input.text); }
                }
                Button {
                    text: "Restart";
                    clicked => { root.runtime_restart(); }
                }
                Button {
                    text: "Stop";
                    clicked => { root.runtime_stop(); }
                }
            }

            Rectangle { height: 1px; background: #d0d0d0; }

            Text { text: "Cloudflare Tunnel"; font-size: 20px; }
            tunnel_name := LineEdit {
                placeholder-text: "Named tunnel, e.g. beamscale-local";
            }
            tunnel_hostname := LineEdit {
                placeholder-text: "Hostname, e.g. dev.example.com";
            }
            HorizontalLayout {
                spacing: 8px;
                Button {
                    text: "Start + route DNS";
                    clicked => {
                        root.tunnel_start(tunnel_name.text, tunnel_hostname.text);
                    }
                }
                Button {
                    text: "Restart";
                    clicked => { root.tunnel_restart(); }
                }
                Button {
                    text: "Stop";
                    clicked => { root.tunnel_stop(); }
                }
            }

            Rectangle { height: 1px; background: #d0d0d0; }

            CheckBox {
                text: "Keep BeamScale alive during lock-screen";
                checked: root.keep_awake;
                toggled() => {
                    root.keep_awake_changed(self.checked);
                }
            }

            Text {
                text: "The GUI delegates this to the daemon. On macOS the daemon uses a long-lived caffeinate inhibitor; the runtime keeps running even if this app closes.";
                wrap: word-wrap;
            }
        }
    }
}

fn main() -> Result<()> {
    let window = MainWindow::new()?;
    let client = DaemonClient::load()?;
    let mutation_in_flight = Arc::new(AtomicBool::new(false));

    {
        let weak = window.as_weak();
        let client = client.clone();
        window.on_refresh(move || {
            spawn_refresh(weak.clone(), client.clone());
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        window.on_doctor(move || {
            spawn_query(weak.clone(), client.clone(), "Doctor", |client| {
                return client.doctor();
            });
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_runtime_start(move |project| {
            let project = project.to_string();
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "runtime start",
                move |client| {
                    return client.runtime_start(&project);
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_runtime_stop(move || {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "runtime stop",
                |client| {
                    return client.runtime_stop();
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_runtime_restart(move || {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "runtime restart",
                |client| {
                    return client.runtime_restart();
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_tunnel_start(move |name, hostname| {
            let name = name.to_string();
            let hostname = hostname.to_string();
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "tunnel start",
                move |client| {
                    return client.tunnel_start(&name, &hostname);
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_tunnel_stop(move || {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "tunnel stop",
                |client| {
                    return client.tunnel_stop();
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_tunnel_restart(move || {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "tunnel restart",
                |client| {
                    return client.tunnel_restart();
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_keep_awake_changed(move |enabled| {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "keep-awake update",
                move |client| {
                    return client.set_keep_awake(enabled);
                },
            );
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        window.on_update_status(move || {
            spawn_query(weak.clone(), client.clone(), "Update check", |client| {
                return client.update_status();
            });
        });
    }

    {
        let weak = window.as_weak();
        let client = client.clone();
        let mutation_in_flight = mutation_in_flight.clone();
        window.on_update_apply(move || {
            spawn_mutation(
                weak.clone(),
                client.clone(),
                mutation_in_flight.clone(),
                "update apply",
                |client| {
                    return client.update_apply();
                },
            );
        });
    }

    spawn_status_poll(window.as_weak(), client.clone());
    window.run()?;
    return Ok(());
}

fn spawn_refresh(weak: slint::Weak<MainWindow>, client: DaemonClient) {
    thread::spawn(move || {
        let result = client.status().map_err(|error| format!("{error:#}"));
        let _ = weak.upgrade_in_event_loop(move |window| {
            apply_status_result(&window, result);
        });
    });
}

fn spawn_query<F>(
    weak: slint::Weak<MainWindow>,
    client: DaemonClient,
    label: &'static str,
    action: F,
) where
    F: FnOnce(&DaemonClient) -> Result<Value> + Send + 'static,
{
    thread::spawn(move || {
        let text = match action(&client) {
            Ok(value) => format_json(&value),
            Err(error) => format!("{label} failed: {error:#}"),
        };
        let _ = weak.upgrade_in_event_loop(move |window| {
            window.set_status_text(text.into());
        });
    });
}

fn spawn_mutation<F>(
    weak: slint::Weak<MainWindow>,
    client: DaemonClient,
    mutation_in_flight: Arc<AtomicBool>,
    label: &'static str,
    action: F,
) where
    F: FnOnce(&DaemonClient) -> Result<Value> + Send + 'static,
{
    if mutation_in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        set_status_text(
            &weak,
            "Another BeamScale daemon mutation is already in progress.".to_owned(),
        );
        return;
    }

    set_status_text(&weak, format!("Applying {label}..."));
    thread::spawn(move || {
        let action_result = action(&client).map_err(|error| format!("{error:#}"));
        let status_result = if action_result.is_ok() {
            client.status().map_err(|error| format!("{error:#}"))
        } else {
            Err(action_result
                .err()
                .unwrap_or_else(|| "unknown daemon mutation failure".to_owned()))
        };
        mutation_in_flight.store(false, Ordering::Release);

        let _ = weak.upgrade_in_event_loop(move |window| match status_result {
            Ok(status) => {
                apply_status_result(&window, Ok(status));
            }
            Err(error) => {
                window.set_status_text(format!("BeamScale {label} failed: {error}").into());
            }
        });
    });
}

fn spawn_status_poll(weak: slint::Weak<MainWindow>, client: DaemonClient) {
    thread::spawn(move || loop {
        let result = client.status().map_err(|error| format!("{error:#}"));
        if weak
            .upgrade_in_event_loop(move |window| apply_status_result(&window, result))
            .is_err()
        {
            break;
        }
        thread::sleep(Duration::from_secs(2));
    });
}

fn apply_status_result(window: &MainWindow, result: Result<Value, String>) {
    match result {
        Ok(value) => {
            window.set_keep_awake(
                value
                    .pointer("/settings/keep_alive_during_lock")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            );
            window.set_status_text(status_summary(&value).into());
        }
        Err(error) => {
            window.set_status_text(
                format!(
                    "Cannot reach beamscale-desktop-daemon. Start the daemon, then refresh.\n\n{error}"
                )
                .into(),
            );
        }
    }
}

fn set_status_text(weak: &slint::Weak<MainWindow>, text: String) {
    if let Some(window) = weak.upgrade() {
        window.set_status_text(text.into());
    }
}

fn status_summary(value: &Value) -> String {
    let runtime = running_label(value.pointer("/runtime/running").and_then(Value::as_bool));
    let tunnel = running_label(value.pointer("/tunnel/running").and_then(Value::as_bool));
    let keep_awake = running_label(value.pointer("/keep_awake/running").and_then(Value::as_bool));
    let version = value
        .get("daemon_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let runtime_desired = value
        .get("runtime_desired")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tunnel_desired = value
        .get("tunnel_desired")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let maintenance = value
        .get("maintenance_in_progress")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dns_routes = value
        .get("known_dns_routes")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    return format!(
        "Daemon v{version} connected\nRuntime: {runtime} (desired: {runtime_desired})\nTunnel: {tunnel} (desired: {tunnel_desired})\nKeep-awake: {keep_awake}\nMaintenance: {maintenance}\nKnown DNS routes: {dns_routes}"
    );
}

fn running_label(value: Option<bool>) -> &'static str {
    if value == Some(true) {
        return "running";
    }

    return "stopped";
}

fn format_json(value: &Value) -> String {
    return serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_summary_surfaces_desired_state_and_maintenance() {
        let status = serde_json::json!({
            "daemon_version": "0.1.0",
            "runtime": {"running": true},
            "runtime_desired": true,
            "tunnel": {"running": false},
            "tunnel_desired": true,
            "keep_awake": {"running": true},
            "maintenance_in_progress": true,
            "known_dns_routes": [
                {"tunnel": "local", "hostname": "dev.example.com"}
            ]
        });

        let summary = status_summary(&status);
        assert!(summary.contains("Runtime: running (desired: true)"));
        assert!(summary.contains("Tunnel: stopped (desired: true)"));
        assert!(summary.contains("Maintenance: true"));
        assert!(summary.contains("Known DNS routes: 1"));
    }

    #[test]
    fn mutation_guard_is_single_flight() {
        let guard = AtomicBool::new(false);
        assert!(
            guard
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
        assert!(
            guard
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        );
        guard.store(false, Ordering::Release);
        assert!(!guard.load(Ordering::Acquire));
    }
}
