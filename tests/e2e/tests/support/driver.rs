// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::OnceCell,
};

pub const LANGUAGES: [&str; 4] = ["python", "rust", "go", "node"];
pub fn python() -> String {
    std::env::var("PARTLY_PYTHON").unwrap_or_else(|_| "python3".into())
}

pub async fn output(mut command: Command, seconds: u64) -> Result<std::process::Output> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let label = format!("{command:?}");
    tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| format!("Command timed out: {label}"))?
        .map_err(Into::into)
}

pub async fn checked(command: Command, seconds: u64) -> Result<std::process::Output> {
    let result = output(command, seconds).await?;
    if !result.status.success() {
        return Err(format!(
            "Command failed ({}):\n{}{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result)
}

pub struct Driver {
    pub _directory: TempDir,
    pub log_path: PathBuf,
    process: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Driver {
    pub fn spawn(mut command: Command, directory: TempDir) -> Result<Self> {
        let log_path = directory.path().join("driver.log");
        let mut process = command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(File::create(&log_path)?)
            .spawn()?;
        let input = process.stdin.take();
        let output = BufReader::new(process.stdout.take().unwrap());
        Ok(Self {
            _directory: directory,
            log_path,
            process,
            input,
            output,
        })
    }

    pub async fn new(language: &str, openfeature: bool) -> Result<Self> {
        if language == "python" {
            verify_python_package().await?;
        }
        let directory = tempfile::tempdir()?;
        let command = if let Some(packages) = std::env::var_os("PARTLY_SDK_PACKAGES") {
            packaged(
                language,
                directory.path(),
                &fs::canonicalize(packages)?,
                openfeature,
            )
            .await?
        } else {
            build_sources(language, openfeature).await?;
            source_command(language, openfeature)?
        };
        Self::spawn(command, directory)
    }

    pub async fn call(&mut self, op: &str, mut arguments: Value) -> Result<Value> {
        self.call_timeout(op, &mut arguments, Duration::from_secs(10))
            .await
    }

    pub async fn call_timeout(
        &mut self,
        op: &str,
        arguments: &mut Value,
        timeout: Duration,
    ) -> Result<Value> {
        arguments["op"] = op.into();
        let result = tokio::time::timeout(timeout, async {
            let input = self.input.as_mut().ok_or("Driver input closed")?;
            input
                .write_all(format!("{arguments}\n").as_bytes())
                .await
                .map_err(|_| format!("SDK driver exited during {op}"))?;
            input.flush().await?;
            let mut line = vec![];
            loop {
                let buffer = self.output.fill_buf().await?;
                if buffer.is_empty() {
                    return Err(format!("SDK driver exited during {op}").into());
                }
                let end = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|position| position + 1);
                let size = end.unwrap_or(buffer.len());
                if line.len() + size > 65536 {
                    return Err(format!("SDK driver response exceeded 64 KiB during {op}").into());
                }
                line.extend_from_slice(&buffer[..size]);
                self.output.consume(size);
                if end.is_some() {
                    break;
                }
            }
            serde_json::from_slice(&line).map_err(|_| {
                Box::<dyn std::error::Error + Send + Sync>::from(format!(
                    "SDK driver returned invalid JSON during {op}"
                ))
            })
        })
        .await
        .unwrap_or_else(|_| Err(format!("SDK driver timed out during {op}").into()));
        result.map_err(|error| {
            format!(
                "{error}\n{}",
                fs::read_to_string(&self.log_path).unwrap_or_default()
            )
            .into()
        })
    }

    pub async fn simple(&mut self, op: &str, expected: Value) -> Result {
        let result = self.call(op, json!({})).await?;
        assert_eq!(result, expected, "Operation: {op}");
        Ok(())
    }

    pub async fn evaluate(&mut self, flag: &str, context: Value) -> Result<Value> {
        self.call("evaluate", json!({"flag":flag,"context":context}))
            .await
    }

    pub async fn build(
        &mut self,
        server: &Server,
        key: &Value,
        interval: f64,
        trusted: bool,
    ) -> Result {
        let mut args = json!({"url":server.url,"key":key["secret"],"environment":key["environment"],"application_id":key["application_id"],"interval":interval,"timeout":1});
        if trusted {
            args["ca_bundle"] = server.ca.as_ref().map(|p| p.display().to_string()).into();
        }
        assert_eq!(self.call("build", args).await?, json!({"ok":true}));
        Ok(())
    }

    pub async fn finish(&mut self) -> Result {
        drop(self.input.take());
        let result = tokio::time::timeout(Duration::from_secs(3), self.process.wait())
            .await
            .map_err(|_| "SDK driver did not shut down after EOF")??;
        if !result.success() {
            return Err(format!(
                "SDK driver exited with {result}: {}",
                fs::read_to_string(&self.log_path)?
            )
            .into());
        }
        Ok(())
    }
}

fn source_command(language: &str, openfeature: bool) -> Result<Command> {
    let drivers = root().join("tests/sdk_contract");
    let mut command = match language {
        "python" => {
            let mut command = Command::new(python());
            command.args(["-I"]).arg(drivers.join(if openfeature {
                "python/openfeature.py"
            } else {
                "python/driver.py"
            }));
            command
        }
        "rust" => Command::new(root().join(if openfeature {
            "target/debug/examples/openfeature-contract"
        } else {
            "target/debug/partly-sdk-contract"
        })),
        "go" => Command::new(root().join(if openfeature {
            "target/e2e-go-openfeature"
        } else {
            "target/e2e-go-driver"
        })),
        "node" => {
            let mut command = Command::new("node");
            command.arg(drivers.join(if openfeature {
                "node/openfeature.mjs"
            } else {
                "node/driver.mjs"
            }));
            command
        }
        _ => return Err(format!("Unknown SDK language: {language}").into()),
    };
    command.current_dir(root());
    Ok(command)
}

static SOURCE_BUILDS: [OnceCell<()>; 5] = [const { OnceCell::const_new() }; 5];
static GO_NATIVE: OnceCell<()> = OnceCell::const_new();
static PYTHON_PACKAGE: OnceCell<()> = OnceCell::const_new();
pub async fn verify_python_package() -> Result {
    PYTHON_PACKAGE.get_or_try_init(||async {
        let mut command=Command::new(python());
        command.args(["-I","-c",r#"import os, sys
assert sys.version_info[:2] == (3, 14), 'Use Python 3.14 for SDK tests'
import partly, partly.openfeature
if os.environ.get('PARTLY_SDK_PACKAGES'):
    from importlib.metadata import distribution
    from pathlib import Path
    package = Path(distribution('partly-sdk').locate_file('partly')).resolve()
    assert Path(partly.__file__).resolve().parent == package, 'Install the release wheel, not an editable SDK'
    assert Path(partly.openfeature.__file__).resolve().parent == package
"#]);
        checked(command,10).await?;Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }).await?;
    Ok(())
}

pub fn go_build_command(directory: &Path, binary: &Path) -> Command {
    let mut command = Command::new("go");
    command
        .current_dir(directory)
        .args(["build", "-a", "-o"])
        .arg(binary)
        .arg(".");
    command
}
async fn build_sources(language: &str, openfeature: bool) -> Result {
    let index = match (language, openfeature) {
        ("python", _) => return Ok(()),
        ("rust", false) => 0,
        ("rust", true) => 1,
        ("go", false) => 2,
        ("go", true) => 3,
        ("node", _) => 4,
        _ => return Err("Unknown SDK language".into()),
    };
    SOURCE_BUILDS[index]
        .get_or_try_init(|| async {
            match language {
                "rust" => {
                    let mut command = Command::new("cargo");
                    command
                        .current_dir(root())
                        .args(["build", "--locked", "--offline", "-p"]);
                    if openfeature {
                        command.args([
                            "partly-rust",
                            "--features",
                            "openfeature",
                            "--example",
                            "openfeature-contract",
                        ]);
                    } else {
                        command.arg("partly-sdk-contract");
                    }
                    checked(command, 180).await?;
                }
                "go" => {
                    GO_NATIVE
                        .get_or_try_init(|| async {
                            let mut command = Command::new("sh");
                            command.current_dir(root()).arg("sdk/go/build.sh");
                            checked(command, 180).await?;
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                        })
                        .await?;
                    let (folder, binary) = if openfeature {
                        ("go_openfeature", "e2e-go-openfeature")
                    } else {
                        ("go", "e2e-go-driver")
                    };
                    checked(
                        go_build_command(
                            &root().join("tests/sdk_contract").join(folder),
                            &root().join("target").join(binary),
                        ),
                        300,
                    )
                    .await?;
                }
                "node" => {
                    let mut command = Command::new("node");
                    command.current_dir(root()).arg("sdk/node/build.js");
                    checked(command, 180).await?;
                }
                _ => unreachable!(),
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    Ok(())
}

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

async fn packaged(
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
            let core = unpack(packages, directory, "partly-core-", ".crate")?;
            let sdk = unpack(packages, directory, "partly-rust-", ".crate")?;
            assert!(fs::read_to_string(core.join("LICENSE"))?.contains("Apache License"));
            assert!(fs::metadata(core.join("NOTICE"))?.len() > 0);
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
            content.push_str(&format!(
                "\n[workspace]\n\n[patch.crates-io]\npartly-core = {{ path = {} }}\n",
                json!(core)
            ));
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
