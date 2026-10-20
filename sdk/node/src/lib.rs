// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use napi::{
    bindgen_prelude::{AsyncTask, Buffer},
    Env, Error, Result, Task,
};
use napi_derive::napi;
use partly_ffi::Snapshot;

#[napi]
pub struct NativeSnapshot(Snapshot);

#[napi]
impl NativeSnapshot {
    #[napi]
    pub fn contains_flag(&self, key: String) -> bool {
        self.0.contains_flag(&key)
    }

    #[napi(getter)]
    pub fn scope(&self) -> String {
        self.0.scope()
    }

    #[napi]
    pub fn evaluate(&self, request: String) -> String {
        self.0.evaluate(request.as_bytes())
    }
}

pub struct Parse(Vec<u8>);

impl Task for Parse {
    type Output = Snapshot;
    type JsValue = NativeSnapshot;

    fn compute(&mut self) -> Result<Self::Output> {
        Snapshot::new(&self.0).map_err(Error::from_reason)
    }
    fn resolve(&mut self, _: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(NativeSnapshot(output))
    }
}

// Policy parsing and regex compilation run on libuv's worker pool.
#[napi]
pub fn parse_policy(body: Buffer) -> AsyncTask<Parse> {
    AsyncTask::new(Parse(body.to_vec()))
}
