#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use flags2env::BundledFlags2Env;

#[derive(Debug)]
struct Outcome {
    command: String,
    unknown_options: usize,
    errors: usize,
    flags: usize,
}

fn policy_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".cli-flags.toml")
}

fn parse(argv: &[&str]) -> Outcome {
    let policy = policy_path();
    let policy = policy.to_str().expect("UTF-8 policy path");
    let parser = BundledFlags2Env::new();
    parser
        .audit_config(Some(policy))
        .expect("exact product flags policy must audit cleanly");
    let parsed = parser
        .parse_structured(
            &argv.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>(),
            Some(policy),
        )
        .expect("flags parser execution");
    Outcome {
        command: parsed.command,
        unknown_options: parsed.unknown_options.len(),
        errors: parsed.errors.len(),
        flags: parsed.flags.len(),
    }
}

fn assert_command(command: &str) {
    let outcome = parse(&["msgint", command]);
    assert_eq!(outcome.errors, 0, "{command} emitted parse errors");
    assert_eq!(
        outcome.unknown_options, 0,
        "{command} emitted unknown options"
    );
    assert_eq!(outcome.command, command);
    assert_eq!(outcome.flags, 0, "command-only CLI exposed flags");
}

fn assert_rejected(argument: &str) {
    let outcome = parse(&["msgint", "identity", argument]);
    assert!(
        outcome.unknown_options != 0 || outcome.errors != 0,
        "credential-shaped or unknown public option was accepted"
    );
}

fn main() {
    assert_command("check-config");
    assert_command("identity");

    for argument in [
        "--token=synthetic",
        "--secret=synthetic",
        "--password=synthetic",
        "--shared-auth-introspect-secret=synthetic",
        "--user-token=synthetic",
        "--unexpected=synthetic",
    ] {
        assert_rejected(argument);
    }

    let policy = std::fs::read_to_string(policy_path()).expect("read policy");
    let lowercase = policy.to_ascii_lowercase();
    assert!(!lowercase.contains("[flags."));
    for sensitive in ["token", "secret", "password", "credential"] {
        assert!(
            !lowercase.contains(sensitive),
            "sensitive terminology leaked into public flags authority"
        );
    }

    println!("msgint-cli PR #2 public flags authority certified");
}
