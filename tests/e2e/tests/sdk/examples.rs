// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::process::Command;

async fn quickstart_command(language: &str, directory: &Path, source: &str) -> Result<Vec<String>> {
    if language == "python" {
        verify_python_package().await?;
    }
    let packages = std::env::var_os("PARTLY_SDK_PACKAGES")
        .map(fs::canonicalize)
        .transpose()?;
    let file = match language {
        "python" => "quickstart.py",
        "rust" => "quickstart.rs",
        "go" => "main.go",
        _ => "quickstart.js",
    };
    fs::write(directory.join(file), source)?;
    let sdk = |prefix, suffix, local| -> Result<PathBuf> {
        if let Some(packages) = &packages {
            unpack(packages, directory, prefix, suffix)
        } else {
            Ok(root().join(local))
        }
    };
    let command = match language {
        "python" => vec![
            python(),
            "-I".into(),
            directory.join(file).display().to_string(),
        ],
        "rust" => {
            let library = sdk("partly-rust-", ".crate", "sdk/rust")?;
            let core = sdk("partly-core-", ".crate", "crates/partly-core")?;
            fs::write(
                directory.join("Cargo.toml"),
                format!(
                    r#"[package]
name = "partly-quickstart-check"
version = "0.0.0"
edition = "2021"
[workspace]
[[bin]]
name = "quickstart"
path = "quickstart.rs"
[dependencies]
partly = {{ package = "partly-rust", path = {}, features = ["openfeature"] }}
open-feature = "0.3.0"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
[patch.crates-io]
partly-core = {{ path = {} }}
"#,
                    json!(library),
                    json!(core)
                ),
            )?;
            let target = root().join("target/sdk-examples");
            let mut build = Command::new("cargo");
            build
                .current_dir(directory)
                .args(["build", "--offline", "--target-dir"])
                .arg(&target);
            checked(build, 300).await?;
            let binary = directory.join("quickstart");
            fs::copy(target.join("debug/quickstart"), &binary)?;
            vec![binary.display().to_string()]
        }
        "go" => {
            let library = sdk("partly-go-", ".tar.gz", "sdk/go")?;
            if packages.is_none() {
                let mut build = Command::new("sh");
                build.current_dir(root()).arg("sdk/go/build.sh");
                checked(build, 180).await?;
            }
            fs::write(
                directory.join("go.mod"),
                fs::read_to_string(root().join("examples/go/go.mod"))?
                    .replace(
                        "../../sdk/go/openfeature",
                        &json!(library.join("openfeature")).to_string(),
                    )
                    .replace("../../sdk/go", &json!(library).to_string()),
            )?;
            fs::copy(root().join("examples/go/go.sum"), directory.join("go.sum"))?;
            let binary = directory.join("quickstart");
            let mut build = Command::new("go");
            build
                .current_dir(directory)
                .args(["build", "-a", "-o"])
                .arg(&binary)
                .arg(".");
            checked(build, 300).await?;
            vec![binary.display().to_string()]
        }
        "node" => {
            if let Some(packages) = packages {
                install_node(directory, &packages, true).await?;
            } else {
                fs::create_dir(directory.join("node_modules"))?;
                std::os::unix::fs::symlink(
                    root().join("sdk/node"),
                    directory.join("node_modules/partly-sdk"),
                )?;
                std::os::unix::fs::symlink(
                    root().join("node_modules/@openfeature"),
                    directory.join("node_modules/@openfeature"),
                )?;
                let mut build = Command::new("node");
                build.current_dir(root()).arg("sdk/node/build.js");
                checked(build, 180).await?;
            }
            vec!["node".into(), directory.join(file).display().to_string()]
        }
        _ => return Err(format!("Unknown language {language}").into()),
    };
    Ok(command)
}

async fn execute(arguments: &[String], directory: &Path) -> Result<std::process::Output> {
    let mut command = Command::new(&arguments[0]);
    command.args(&arguments[1..]).current_dir(directory);
    output(command, 15).await
}

#[tokio::test]
async fn language_quickstarts() -> Result {
    for language in LANGUAGES {
        let (server, admin) = fixture().await?;
        let key = admin.key("development").await?;
        let (file, endpoint) = match language {
            "python" => ("quickstart.py", "python.py"),
            "rust" => ("quickstart.rs", "rust.rs"),
            "go" => ("main.go", "go.go"),
            _ => ("quickstart.js", "node.js"),
        };
        let response = admin
            .request(reqwest::Method::GET, &format!("/docs/{endpoint}"))
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        let source = response.text().await?;
        assert_eq!(
            source,
            fs::read_to_string(root().join("examples").join(language).join(file))?
        );
        let source = source
            .replace("\"http://127.0.0.1:8080\"", &json!(server.url).to_string())
            .replace("\"YOUR_API_KEY\"", &key["secret"].to_string())
            .replace("YOUR_APPLICATION_ID", text(&key["application_id"]));
        let directory = tempfile::tempdir()?;
        let command = quickstart_command(language, directory.path(), &source).await?;
        let result = execute(&command, directory.path()).await?;
        assert!(
            !result.status.success() && result.stdout.is_empty(),
            "{language}: unknown flag used fallback"
        );
        assert!(!String::from_utf8_lossy(&result.stderr).contains(text(&key["secret"])));
        for (mode, expected) in [
            ("enabled", "Use the new checkout"),
            ("disabled", "Use the current checkout"),
        ] {
            admin
                .flag("development", "new_checkout", json!({"mode":mode}))
                .await?;
            let result = execute(&command, directory.path()).await?;
            assert!(
                result.status.success(),
                "{language}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout)
                    .lines()
                    .collect::<Vec<_>>(),
                [expected, expected]
            );
            for bytes in [&result.stdout, &result.stderr] {
                assert!(!String::from_utf8_lossy(bytes).contains(text(&key["secret"])));
            }
        }
        admin.flag("development","new_checkout",json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18}]]})).await?;
        let result = execute(&command, directory.path()).await?;
        assert!(!result.status.success() && result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(text(&key["secret"])));
        admin
            .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
            .await?;
        let result = execute(&command, directory.path()).await?;
        assert!(!result.status.success() && result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(text(&key["secret"])));
    }
    Ok(())
}

#[tokio::test]
async fn python_quickstart_configuration_failures() -> Result {
    for (scenario, expected) in [
        ("wrong_context_type", "INVALID_CONTEXT"),
        ("invalid_key", "SDK key rejected"),
        ("empty_key", "Supply a non-empty ASCII SDK secret key"),
        ("invalid_url", "Supply a control-plane URL"),
        ("offline", "Policy request failed"),
    ] {
        let (mut server, admin) = fixture().await?;
        let key = admin.key("development").await?;
        let flag = if scenario == "wrong_context_type" {
            json!({"mode":"custom","action":"enable","groups":[[{"k":"id","op":"ge","v":1}]]})
        } else {
            json!({"mode":"enabled"})
        };
        admin.flag("development", "new_checkout", flag).await?;
        let api_key = match scenario {
            "invalid_key" => "invalid",
            "empty_key" => "",
            _ => text(&key["secret"]),
        };
        let url = if scenario == "invalid_url" {
            "invalid"
        } else {
            &server.url
        };
        let source = fs::read_to_string(root().join("examples/python/quickstart.py"))?
            .replace("\"http://127.0.0.1:8080\"", &json!(url).to_string())
            .replace("\"YOUR_API_KEY\"", &json!(api_key).to_string())
            .replace("YOUR_APPLICATION_ID", text(&key["application_id"]));
        let directory = tempfile::tempdir()?;
        let command = quickstart_command("python", directory.path(), &source).await?;
        if scenario == "offline" {
            server.stop().await?;
        }
        let result = execute(&command, directory.path()).await?;
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains("Use the"));
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains(expected), "{scenario}: {error}");
        assert!(!error.contains(text(&key["secret"])));
    }
    Ok(())
}

#[tokio::test]
async fn unknown_docs_example_returns_not_found() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    assert_eq!(
        server
            .api(false)?
            .request(reqwest::Method::GET, "/docs/Cargo.toml")
            .send()
            .await?
            .status(),
        404
    );
    Ok(())
}

#[tokio::test]
async fn python_quickstart_evaluates_offline_and_closes() -> Result {
    let (mut server, admin) = fixture().await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    let key = admin.key("development").await?;
    let mut driver = Driver::new("python", false).await?;
    assert_eq!(driver.call("quickstart",json!({"path":root().join("examples/python/quickstart.py"),"url":server.url,"key":key["secret"],"application_id":key["application_id"]})).await?,json!({"ready":true}));
    server.stop().await?;
    assert_eq!(
        driver.call("continue", json!({})).await?,
        json!({"lines":["Use the new checkout","Use the new checkout"]})
    );
    driver.finish().await
}
