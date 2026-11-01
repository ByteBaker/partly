// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use tokio::sync::OnceCell;

pub(super) fn source_command(language: &str, openfeature: bool) -> Result<Command> {
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
pub(super) async fn build_sources(language: &str, openfeature: bool) -> Result {
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
