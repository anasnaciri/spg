use std::process::Command;

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn help_flag_prints_usage_and_exits_successfully() {
    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_spg"))
            .arg(flag)
            .output()
            .expect("failed to spawn spg");

        assert!(output.status.success(), "{flag} should exit 0");
        let stdout = String::from_utf8(output.stdout).expect("stdout was not UTF-8");
        assert!(
            stdout.contains("Interactive Spring Initializr project generator"),
            "{flag} should print the about line; got: {stdout}"
        );
        assert!(
            stdout.contains("init") && stdout.contains("deps"),
            "{flag} should list subcommands; got: {stdout}"
        );
    }
}

#[test]
fn version_flag_prints_crate_version_and_exits_successfully() {
    for flag in ["--version", "-v"] {
        let output = Command::new(env!("CARGO_BIN_EXE_spg"))
            .arg(flag)
            .output()
            .expect("failed to spawn spg");

        assert!(output.status.success(), "{flag} should exit 0");
        let stdout = String::from_utf8(output.stdout).expect("stdout was not UTF-8");
        assert!(
            stdout.contains(PKG_VERSION),
            "{flag} should print the crate version {PKG_VERSION}; got: {stdout}"
        );
    }
}

#[test]
fn no_arguments_prints_help_and_exits_with_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_spg"))
        .output()
        .expect("failed to spawn spg");

    assert!(
        !output.status.success(),
        "running without a subcommand should be a usage error"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not UTF-8");
    assert!(
        stderr.contains("Usage:") || stderr.contains("usage:"),
        "stderr should include the usage line; got: {stderr}"
    );
}

#[test]
fn unknown_subcommand_exits_with_error_and_mentions_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_spg"))
        .arg("not-a-real-subcommand")
        .output()
        .expect("failed to spawn spg");

    assert!(
        !output.status.success(),
        "unknown subcommand should fail parsing"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not UTF-8");
    assert!(
        stderr.contains("Usage:") || stderr.contains("unrecognized"),
        "stderr should help the user recover; got: {stderr}"
    );
}
