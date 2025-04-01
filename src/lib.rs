/*!
# katex-gdef-v8

A Rust library that utilizes KaTeX (v0.16.21) through the V8 engine to render LaTeX math expressions to HTML.

## Features

* **Fast Processing**: Rapid initialization and rendering using V8 snapshots
* **Single Instance**: Reuse of a single KaTeX instance to minimize loading delays (though not optimized for parallel processing)
* **Macro Support**: Collect and reuse macros defined with `\gdef` and similar commands (Note: depends on KaTeX v0.16.21 internal representation)
* **Caching Capability**: Cache V8 snapshots to the filesystem to reduce startup time

## Installation

Add this to your Cargo.toml:

```toml
[dependencies]
katex-gdef-v8 = "0.1.0"
```

## Usage

### Basic Example

```rust
use katex_gdef_v8::render;

fn main() {
    // KaTeX is initialized automatically on first call
    let html = render(r"E = mc^2").unwrap();
    println!("{}", html);
}
```

### Using Options and Macros

```rust
use katex_gdef_v8::{render_with_opts, Options, KatexOutput};
use std::collections::BTreeMap;
use std::borrow::Cow;

fn main() {
    let mut macros = BTreeMap::new();

    // Set custom options
    let options = Options {
        display_mode: true,
        output: KatexOutput::HtmlAndMathml,
        error_color: Cow::Borrowed("#ff0000"),
        ..Default::default()
    };

    // Render first equation (defining macros)
    let html1 = render_with_opts(
        r"\gdef\myvar{x} \myvar^2 + \myvar = 0",
        &options,
        &mut macros
    ).unwrap();
    println!("HTML 1: {}", html1);

    // Use previously defined macros in second equation
    let html2 = render_with_opts(
        r"\myvar^3",
        &options,
        &mut macros
    ).unwrap();
    println!("HTML 2: {}", html2);
}
```

### Setting Up Cache

```rust
use katex_gdef_v8::{set_cache, render};
use std::path::Path;

fn main() {
    // Set path to cache V8 snapshot
    set_cache(Path::new("./katex-cache"));

    // Subsequent renderings will be faster
    let html = render(r"E = mc^2").unwrap();
    println!("{}", html);
}
```

## Comparison with `katex-rs`

* **Macro collection and reuse**: Ability to reuse macros defined in equations in subsequent renderings (main differentiating feature)
* **Caching capability**: Fast initialization with V8 snapshots
* **Single-thread optimization**: Shared KaTeX instance in one worker thread (though not suitable for parallel processing)

Note that `katex-rs` supports more JavaScript engines (duktape, wasm-js, etc.), making it more versatile in that respect.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
*/

use deno_core::{JsRuntime, RuntimeOptions, error::CoreError, v8};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
pub static KATEX_VERSION: &str = "0.16.21";
static KATEX_CODE: &str = concat!(
    include_str!("./katex.min.js"),
    r#"function renderToStringAndMacros(input) {
        try {
            const html = katex.renderToString(
                input.latex,
                Object.assign({}, input.options, { macros: input.macros })
            );
            for (let key in input.macros) if (typeof input.macros[key] !== "string") {
                input.macros[key] = input.macros[key].tokens.map(token => token.text).reverse().join("");
            }
            return JSON.stringify({ html: html, macros: input.macros });
        } catch (e) {
            if (e instanceof katex.ParseError) {
                for (let key in input.macros) if (typeof input.macros[key] !== "string") {
                    input.macros[key] = input.macros[key].tokens.map(token => token.text).reverse().join("");
                }
                return JSON.stringify({ error: e.message, macros: input.macros });
            } else {
                throw e;
            }
        }
    }"#
);

#[derive(Debug, Serialize)]
pub struct Input<'a> {
    pub latex: &'a str,
    pub options: &'a Options,
    pub macros: &'a mut BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub display_mode: bool,
    pub output: KatexOutput,
    pub leqno: bool,
    pub fleqn: bool,
    pub throw_on_error: bool,
    pub error_color: Cow<'static, str>,
    pub min_rule_thickness: Option<f64>,
    pub color_is_text_color: bool,
    pub max_size: f64,
    pub max_expand: i32,
    pub strict: Option<bool>,
    pub trust: bool,
    pub global_group: bool,
}
impl Default for Options {
    fn default() -> Self {
        Options {
            display_mode: false,
            output: KatexOutput::HtmlAndMathml,
            leqno: false,
            fleqn: false,
            throw_on_error: true,
            error_color: "#cc0000".into(),
            min_rule_thickness: None,
            color_is_text_color: false,
            max_size: std::f64::INFINITY,
            max_expand: 1000,
            strict: None,
            trust: false,
            global_group: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KatexOutput {
    Html,
    Mathml,
    HtmlAndMathml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Output {
    Success { html: String, macros: BTreeMap<String, String> },
    Error { error: String, macros: BTreeMap<String, String> },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroExpansion {
    #[serde(default)]
    pub delimiters: Vec<Vec<String>>,
    #[serde(default)]
    pub num_args: i32,
    #[serde(default)]
    pub tokens: Vec<MacroToken>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroToken {
    pub text: String,
    pub noexpand: Option<bool>,
    pub treat_as_relax: Option<bool>,
}

pub struct KatexWorker(Sender<(String, Sender<Result<String, Error>>)>);
pub static KATEX_WORKER: OnceCell<KatexWorker> = OnceCell::new();

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("V8 Error: {0}")]
    V8Error(#[from] CoreError),
    #[error("Recv Error: {0}")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Send Error")]
    SendError,
    #[error("JSON Error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("KaTeX Error: math: {latex}, macros: {macros:?}, error: {message}")]
    KaTeXError { message: String, latex: String, macros: BTreeMap<String, String> },
}

pub fn set_cache(path: impl AsRef<Path>) {
    init_katex_worker(Some(path.as_ref().to_path_buf()));
}

fn init_katex_worker(cache: Option<PathBuf>) {
    if KATEX_WORKER.get().is_some() {
        return;
    }
    let (tx, rx): (Sender<(String, Sender<Result<String, Error>>)>, Receiver<(String, Sender<Result<String, Error>>)>) = mpsc::channel();
    thread::spawn(move || {
        let mut runtime = get_runtime(cache);
        for (katex_input, sender) in rx {
            sender.send(katex_work(&mut runtime, katex_input)).unwrap();
        }
    });
    KATEX_WORKER.set(KatexWorker(tx)).ok().unwrap();
}

fn katex_work(runtime: &mut JsRuntime, katex_input: String) -> Result<String, Error> {
    let code = format!("renderToStringAndMacros({})", katex_input);
    let result = runtime.execute_script("katex", code)?;
    let scope = &mut runtime.handle_scope();
    let local_result = v8::Local::new(scope, result);
    Ok(local_result.to_rust_string_lossy(scope))
}

fn get_runtime(cache: Option<PathBuf>) -> JsRuntime {
    if let Some(cache) = cache {
        if let Some(snapshot) = get_snapshot(cache) {
            let mut options = RuntimeOptions::default();
            options.startup_snapshot = Some(snapshot);
            return deno_core::JsRuntime::new(options);
        }
    }
    let mut rtm = deno_core::JsRuntime::new(RuntimeOptions::default());
    rtm.execute_script("katex", KATEX_CODE).unwrap();
    rtm
}

// 諸々のエラーを無視
fn get_snapshot(cache: PathBuf) -> Option<&'static [u8]> {
    if cache.exists() {
        let mut file = std::fs::File::open(cache).ok()?;
        let mut bytecode = Vec::new();
        file.read_to_end(&mut bytecode).ok()?;
        Some(Box::leak(bytecode.into()))
    } else {
        let mut rtm = deno_core::JsRuntimeForSnapshot::new(RuntimeOptions::default());
        rtm.execute_script("katex", KATEX_CODE).ok()?;
        let snapshot = rtm.snapshot();
        let mut file = std::fs::File::create(cache).ok()?;
        file.write_all(&snapshot).ok()?;
        Some(Box::leak(snapshot))
    }
}

pub fn render(latex: &str) -> Result<String, Error> {
    render_with_opts(latex, &Default::default(), &mut BTreeMap::new())
}

pub fn render_with_opts(latex: &str, options: &Options, macros: &mut BTreeMap<String, String>) -> Result<String, Error> {
    let (tx, rx) = mpsc::channel();
    let Some(worker) = KATEX_WORKER.get() else {
        init_katex_worker(None);
        return render_with_opts(latex, options, macros);
    };

    worker.0.send((serde_json::to_string(&Input { latex, options, macros })?, tx)).map_err(|_| Error::SendError)?;
    let out_str = rx.recv()??;
    match serde_json::from_str(&out_str)? {
        Output::Success { html, macros: macros_value } => {
            *macros = macros_value;
            Ok(html)
        }
        Output::Error { error, macros: macros_value } => {
            Err(Error::KaTeXError { message: error, latex: latex.to_string(), macros: macros_value })
        }
    }
}
