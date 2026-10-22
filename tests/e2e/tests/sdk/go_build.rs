// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use serde_json::json;
use std::fs;
use tokio::process::Command;

const NATIVE: &str = r##"
use std::ffi::{c_char,c_int,CString};
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_new(_: *const u8,_: usize,scope: *mut *mut c_char)->*mut u8 {
    *scope=CString::new(r#"["default","development","APPLICATION_ID"]"#).unwrap().into_raw();
    Box::into_raw(Box::new(0))
}
#[no_mangle]
pub extern "C" fn partly_snapshot_evaluate(_: *const u8,_: *const u8,_: usize)->*mut c_char {
    CString::new(r#"{"decision":"DECISION"}"#).unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn partly_snapshot_has_flag(_: *const u8,_: *const u8,_: usize)->c_int {1}
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_free(snapshot: *mut u8) {drop(Box::from_raw(snapshot));}
#[no_mangle]
pub unsafe extern "C" fn partly_string_free(value: *mut c_char) {drop(CString::from_raw(value));}
"##;

#[tokio::test]
async fn contract_links_rebuilt_native_archive() -> Result {
    let (server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    let directory = tempfile::tempdir()?;
    let workspace = directory.path();
    for path in ["sdk/go", "tests/sdk_contract/go"] {
        copy_directory(&root().join(path), &workspace.join(path))?;
    }
    fs::write(workspace.join("Cargo.toml"),"[package]\nname = \"partly-ffi\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[lib]\npath = \"native.rs\"\ncrate-type = [\"staticlib\"]\n")?;
    fs::write(workspace.join("native.rs"), NATIVE)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(["generate-lockfile", "--offline"]);
    checked(command, 30).await?;
    for decision in ["enabled", "disabled"] {
        fs::write(
            workspace.join("native.rs"),
            NATIVE
                .replace("DECISION", decision)
                .replace("APPLICATION_ID", text(&key["application_id"])),
        )?;
        let mut command = Command::new("sh");
        command.current_dir(workspace).arg("sdk/go/build.sh");
        checked(command, 180).await?;
        let entry = workspace.join("driver");
        checked(
            go_build_command(&workspace.join("tests/sdk_contract/go"), &entry),
            300,
        )
        .await?;
        let mut driver = Driver::spawn(Command::new(entry), tempfile::tempdir()?)?;
        driver.build(&server, &key, 3600.0, true).await?;
        driver.simple("init", json!({"ok":true})).await?;
        assert_eq!(
            driver.evaluate("flag", json!({})).await?,
            json!({"decision":decision})
        );
        driver.simple("close", json!({"ok":true})).await?;
        driver.finish().await?;
    }
    Ok(())
}
