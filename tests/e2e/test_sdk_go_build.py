# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""The contract driver must link changed native code even when Go source is unchanged."""

import shutil
import subprocess
from contextlib import contextmanager

import test_sdk_contract as contract
from conftest import ROOT, make_key

NATIVE_SOURCE = r'''
// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0
use std::ffi::{c_char, c_int, CString};

#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_new(_: *const u8, _: usize, scope: *mut *mut c_char) -> *mut u8 {
    *scope = CString::new("scope").unwrap().into_raw();
    Box::into_raw(Box::new(0))
}
#[no_mangle]
pub extern "C" fn partly_snapshot_evaluate(_: *const u8, _: *const u8, _: usize) -> *mut c_char {
    CString::new(r#"{"decision":"DECISION"}"#).unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn partly_snapshot_has_flag(_: *const u8, _: *const u8, _: usize) -> c_int {
    1
}
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_free(snapshot: *mut u8) {
    drop(Box::from_raw(snapshot));
}
#[no_mangle]
pub unsafe extern "C" fn partly_string_free(value: *mut c_char) {
    drop(CString::from_raw(value));
}
'''


def test_go_contract_links_rebuilt_native_archive(tmp_path, monkeypatch, server, admin):
    # Keep the real Go package and driver, with a tiny native library to isolate linking.
    for directory in ("sdk/go", "tests/sdk_contract/go"):
        shutil.copytree(ROOT / directory, tmp_path / directory)
    (tmp_path / "Cargo.toml").write_text("""
[package]
name = "partly-ffi"
version = "0.0.0"
edition = "2021"
[lib]
path = "native.rs"
crate-type = ["staticlib"]
""")
    native = tmp_path / "native.rs"
    native.write_text(NATIVE_SOURCE.replace("DECISION", "enabled"))
    subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=tmp_path,
                   check=True, capture_output=True, timeout=30)
    monkeypatch.setattr(contract, "ROOT", tmp_path)
    monkeypatch.setattr(contract, "DRIVERS", tmp_path / "tests/sdk_contract")
    _, key = make_key(admin)
    run = tmp_path / "driver-run"
    run.mkdir()

    for decision in ("enabled", "disabled"):
        native.write_text(NATIVE_SOURCE.replace("DECISION", decision))
        with contextmanager(contract.driver.__wrapped__)("go", run) as driver:
            assert driver.call("build", url=server.url, key=key, interval=3600, timeout=1) == {"ok": True}
            assert driver.call("init") == {"ok": True}
            assert driver.call("evaluate", flag="flag", context={}) == {"decision": decision}
            assert driver.call("close") == {"ok": True}
            driver.finish()
