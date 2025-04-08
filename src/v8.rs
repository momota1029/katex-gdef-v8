use std::{
    io::{Read as _, Write as _},
    path::Path,
};

use crate::{Core, Input, Output};

pub(crate) type Engine = deno_core::JsRuntime;
pub type Error = V8Error;

#[derive(Debug, thiserror::Error)]
pub enum V8Error {
    #[error("Runtime Error: {0}")]
    Runtime(#[from] deno_core::error::CoreError),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Core for deno_core::JsRuntime {
    type Error = V8Error;
    fn new() -> Result<Self, Self::Error> {
        let mut rtm = deno_core::JsRuntime::new(deno_core::RuntimeOptions::default());
        rtm.execute_script("katex", crate::KATEX_CODE)?;
        Ok(rtm)
    }
    fn new_with_snapshot(path: &Path) -> Result<Self, Self::Error> {
        let Ok(snapshot) = get_snapshot(path) else { return Core::new() };
        let mut options = deno_core::RuntimeOptions::default();
        options.startup_snapshot = Some(snapshot);
        return Ok(deno_core::JsRuntime::new(options));
    }
    fn exec(&mut self, code: Input) -> Result<Output, Self::Error> {
        let result = self.execute_script("katex", format!("renderToStringAndMacros({})", serde_json::to_string(&code)?))?;
        let scope = &mut self.handle_scope();
        let local_result = deno_core::v8::Local::new(scope, result);
        Ok(serde_json::from_str(&local_result.to_rust_string_lossy(scope))?)
    }
}

fn get_snapshot(cache: &Path) -> Result<&'static [u8], V8Error> {
    if cache.exists() {
        let mut file = std::fs::File::open(cache)?;
        let mut bytecode = Vec::new();
        file.read_to_end(&mut bytecode)?;
        Ok(Box::leak(bytecode.into()))
    } else {
        let mut rtm = deno_core::JsRuntimeForSnapshot::new(deno_core::RuntimeOptions::default());
        rtm.execute_script("katex", crate::KATEX_CODE)?;
        let snapshot = rtm.snapshot();
        let mut file = std::fs::File::create(cache)?;
        file.write_all(&snapshot)?;
        Ok(Box::leak(snapshot))
    }
}
