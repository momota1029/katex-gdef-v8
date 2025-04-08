use std::io::Read as _;

use crate::{Core, Input, Output};

use quickjs_rusty as qjs;
pub use quickjs_rusty::Context;

pub(crate) type Engine = qjs::Context;
pub type Error = QJSError;

#[derive(Debug, thiserror::Error)]
pub enum QJSError {
    #[error("Execution Error: {0}")]
    Execution(#[from] qjs::ExecutionError),
    #[error("Context Error: {0}")]
    Context(#[from] qjs::ContextError),
    #[error("Value Error: {0}")]
    Value(#[from] qjs::ValueError),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Core for qjs::Context {
    type Error = QJSError;

    fn new() -> Result<Self, Self::Error> {
        let ctx = Context::new(None)?;
        ctx.eval(crate::KATEX_CODE, false)?;
        Ok(ctx)
    }

    fn new_with_snapshot(cache: &std::path::Path) -> Result<Self, Self::Error> {
        let ctx = Context::new(None)?;
        let compiled_katex = if cache.exists() {
            let mut file = std::fs::File::open(cache)?;
            let mut bytecode = Vec::new();
            file.read_to_end(&mut bytecode)?;
            unsafe { qjs::compile::from_bytecode(ctx.context_raw(), &bytecode)?.try_into_compiled_function()? }
        } else {
            unsafe {
                let compiled_katex =
                    qjs::compile::compile(ctx.context_raw(), crate::KATEX_CODE, "katex.min.js")?.try_into_compiled_function()?;
                std::fs::write(cache, qjs::compile::to_bytecode(ctx.context_raw(), &compiled_katex))?;
                compiled_katex
            }
        };
        qjs::compile::run_compiled_function(&compiled_katex)?;
        Ok(ctx)
    }

    fn exec(&mut self, input: Input) -> Result<Output, Self::Error> {
        let result = self.eval(&format!("renderToStringAndMacros({})", serde_json::to_string(&input)?), false)?;
        Ok(serde_json::from_str(&result.to_string()?)?)
    }
}
