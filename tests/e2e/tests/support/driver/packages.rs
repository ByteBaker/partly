// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

pub fn copy_directory(source: &Path, destination: &Path) -> Result {
    fs::create_dir_all(destination)?;
    for item in fs::read_dir(source)? {
        let item = item?;
        let target = destination.join(item.file_name());
        if item.file_type()?.is_dir() {
            copy_directory(&item.path(), &target)?;
        } else {
            fs::copy(item.path(), target)?;
        }
    }
    Ok(())
}

pub fn archive(packages: &Path, prefix: &str, suffix: &str) -> Result<PathBuf> {
    let matches: Vec<_> = fs::read_dir(packages)?
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with(prefix) && name.ends_with(suffix)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "Expected one {prefix}*{suffix} in {}",
        packages.display()
    );
    Ok(matches[0].clone())
}

pub fn unpack(packages: &Path, directory: &Path, prefix: &str, suffix: &str) -> Result<PathBuf> {
    let source = archive(packages, prefix, suffix)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(File::open(source)?));
    let mut first = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if first.is_none() {
            first = path
                .components()
                .next()
                .map(|part| directory.join(part.as_os_str()));
        }
        assert!(
            entry.unpack_in(directory)?,
            "Unsafe package path: {}",
            path.display()
        );
    }
    first.ok_or_else(|| "Empty SDK archive".into())
}

fn notices(package: &Path, compiled: bool) -> Result {
    assert!(fs::read_to_string(package.join("LICENSE"))?.contains("Apache License"));
    for name in [
        "NOTICE",
        "THIRD_PARTY_NOTICES.txt",
        "RUST_STDLIB_NOTICES.html",
    ]
    .iter()
    .take(if compiled { 3 } else { 2 })
    {
        assert!(
            fs::metadata(package.join(name))?.len() > 0,
            "Missing notice {name}"
        );
    }
    Ok(())
}

pub async fn install_node(directory: &Path, packages: &Path, openfeature: bool) -> Result {
    let package = archive(packages, "partly-node-", ".tgz")?;
    let mut command = Command::new("npm");
    command.current_dir(directory).arg("install");
    if !openfeature {
        command.arg("--offline");
    }
    command
        .args(["--ignore-scripts", "--no-audit", "--no-fund", "--prefix"])
        .arg(directory)
        .arg(package);
    if openfeature {
        let manifest: Value = serde_json::from_slice(&fs::read(root().join("package.json"))?)?;
        command.arg(format!(
            "@openfeature/server-sdk@{}",
            text(&manifest["devDependencies"]["@openfeature/server-sdk"])
        ));
    }
    checked(command, 120).await?;
    Ok(())
}

pub(super) async fn packaged(
    language: &str,
    directory: &Path,
    packages: &Path,
    openfeature: bool,
) -> Result<Command> {
    static BUILD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    // Consumers share Cargo's target cache; retain the lock until their executable is copied.
    let _guard = BUILD.lock().await;
    let drivers = root().join("tests/sdk_contract");
    let mut command = match language {
        "python" => {
            let entry = directory.join("driver.py");
            fs::copy(
                drivers.join(if openfeature {
                    "python/openfeature.py"
                } else {
                    "python/driver.py"
                }),
                &entry,
            )?;
            let mut command = Command::new(python());
            command.arg("-I").arg(entry);
            command
        }
        "rust" => {
            let sdk = unpack(packages, directory, "partly-rust-", ".crate")?;
            notices(&sdk, false)?;
            let app = directory.join("rust");
            copy_directory(&drivers.join("rust"), &app)?;
            let manifest = app.join("Cargo.toml");
            let mut content = fs::read_to_string(&manifest)?;
            if openfeature {
                fs::copy(app.join("openfeature.rs"), app.join("src/main.rs"))?;
                content = content.replace(
                    "path = \"../../../sdk/rust\"",
                    "path = \"../../../sdk/rust\", features = [\"openfeature\"]",
                );
                content.push_str("\nopen-feature = \"0.3.0\"\n");
            }
            content = content.replace(
                "path = \"../../../sdk/rust\"",
                &format!("path = {}", json!(sdk)),
            );
            content.push_str("\n[workspace]\n");
            fs::write(manifest, content)?;
            fs::copy(sdk.join("Cargo.lock"), app.join("Cargo.lock"))?;
            let target = root().join("target/e2e-packaged-rust");
            let mut build = Command::new("cargo");
            build
                .current_dir(&app)
                .args(["build", "--offline", "--target-dir"])
                .arg(&target);
            checked(build, 300).await?;
            // Copy the executable before another isolated consumer rebuilds this target.
            let entry = directory.join("rust-driver");
            fs::copy(target.join("debug/partly-sdk-contract"), &entry)?;
            Command::new(entry)
        }
        "go" => {
            let sdk = unpack(packages, directory, "partly-go-", ".tar.gz")?;
            notices(&sdk, true)?;
            let app = directory.join("go");
            copy_directory(
                &drivers.join(if openfeature { "go_openfeature" } else { "go" }),
                &app,
            )?;
            let manifest = app.join("go.mod");
            fs::write(
                &manifest,
                fs::read_to_string(&manifest)?
                    .replace(
                        "../../../sdk/go/openfeature",
                        &json!(sdk.join("openfeature")).to_string(),
                    )
                    .replace("../../../sdk/go", &json!(sdk).to_string()),
            )?;
            let entry = app.join("driver");
            checked(go_build_command(&app, &entry), 300).await?;
            Command::new(entry)
        }
        "node" => {
            install_node(directory, packages, openfeature).await?;
            notices(&directory.join("node_modules/partly-sdk"), true)?;
            let entry = directory.join("driver.mjs");
            fs::write(
                &entry,
                fs::read_to_string(drivers.join(if openfeature {
                    "node/openfeature.mjs"
                } else {
                    "node/driver.mjs"
                }))?
                .replace("../../../sdk/node", "partly-sdk"),
            )?;
            let mut command = Command::new("node");
            command.arg(entry);
            command
        }
        _ => return Err(format!("Unknown SDK language {language}").into()),
    };
    command.current_dir(directory);
    Ok(command)
}
