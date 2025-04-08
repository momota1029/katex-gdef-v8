/*!
# katex-gdef-v8

A Rust library that utilizes KaTeX (v0.16.21) through the V8 engine to render LaTeX math expressions to HTML.

## Features

* **Fast Processing**: Rapid initialization and rendering using V8 snapshots
* **Single Instance**: Reuse of a single KaTeX instance to minimize loading delays (though not optimized for parallel processing)
* **Macro Support**: Collect and reuse macros defined with `\gdef` and similar commands (Note: depends on KaTeX v0.16.21 internal representation)
* **Caching Capability**: Cache V8 snapshots to the filesystem to reduce startup time
* **Font Detection**: Analyze rendered HTML to detect which KaTeX fonts are used

## Installation

Add this to your Cargo.toml:

```toml
[dependencies]
katex-gdef-v8 = "0.1.4"
```

## Usage

### Basic Example

```rust
use katex_gdef_v8::render;

// KaTeX is initialized automatically on first call
let html = render(r"E = mc^2").unwrap();
println!("{}", html);
```

### Using Options and Macros

```rust
use katex_gdef_v8::{render_with_opts, Options, KatexOutput};
use std::collections::BTreeMap;
use std::borrow::Cow;

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
```

### Font Detection

The library can analyze rendered KaTeX HTML to determine which fonts are used:

```rust
use katex_gdef_v8::{render, font_extract};
use std::collections::HashSet;

// Render a LaTeX expression
let html = render(r"\mathcal{F}(x) = \int_{-\infty}^{\infty} f(x) e^{-2\pi i x \xi} dx").unwrap();

// Extract font information
let used_fonts = font_extract(&html);

// Check if specific fonts are used
println!("Is empty: {}", used_fonts.is_empty());

// Iterate through used fonts
for font_name in used_fonts.clone() {
    // Each font_name is the base name (e.g., "KaTeX_Math-Italic")
    // To get the complete font file name, add file extension:
    println!("Font file: {}.woff2", font_name);
}

// Collect all font names into a HashSet
let font_set: HashSet<&str> = used_fonts.collect();

// Example assertion for testing
assert_eq!(
    font_set,
    HashSet::from([
        "KaTeX_Main-Regular",
        "KaTeX_Math-Italic",
        "KaTeX_Size1-Regular",
        "KaTeX_Caligraphic-Regular"
    ])
);
```

The `FontFlags` struct provides detailed information about all KaTeX fonts used in the rendered output, which can be useful for:

- Optimizing font loading by only including required fonts (each font name can be used with extensions like `.woff2`, `.woff`, `.ttf`)
- Selective font preloading in web applications

### Setting Up Cache

```rust
use katex_gdef_v8::{set_cache, render};
use std::path::Path;

// Set path to cache V8 snapshot
set_cache(Path::new("./katex-cache"));

// Subsequent renderings will be faster
let html = render(r"E = mc^2").unwrap();
println!("{}", html);
```

## Comparison with `katex-rs`

* **Macro collection and reuse**: Ability to reuse macros defined in equations in subsequent renderings (main differentiating feature)
* **Caching capability**: Fast initialization with V8 snapshots
* **Single-thread optimization**: Shared KaTeX instance in one worker thread (though not suitable for parallel processing)
* **Font analysis**: Ability to detect which KaTeX fonts are used in the rendered output

Note that `katex-rs` supports more JavaScript engines (duktape, wasm-js, etc.), making it more versatile in that respect.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
*/

mod font;

#[cfg(feature = "v8")]
#[cfg(not(feature = "qjs"))]
mod v8;
#[cfg(feature = "v8")]
#[cfg(not(feature = "qjs"))]
type Engine = v8::Engine;
#[cfg(feature = "v8")]
#[cfg(not(feature = "qjs"))]
pub type JSError = v8::Error;

#[cfg(feature = "qjs")]
mod qjs;
#[cfg(feature = "qjs")]
type Engine = qjs::Engine;
#[cfg(feature = "qjs")]
pub type JSError = qjs::Error;

#[cfg(not(any(feature = "v8", feature = "qjs")))]
compile_error!("At least one of the features 'v8' or 'qjs' must be enabled");

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::BTreeMap,
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

#[derive(Clone, Debug, Serialize)]
struct Input {
    pub latex: String,
    pub options: Options,
    pub macros: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KatexOutput {
    Html,
    Mathml,
    HtmlAndMathml,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Output {
    Success { html: String, macros: BTreeMap<String, String> },
    Error { error: String, macros: BTreeMap<String, String> },
}

struct KatexWorker(Sender<(Input, Sender<Result<Output, Error>>)>);
static KATEX_WORKER: OnceCell<KatexWorker> = OnceCell::new();

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("JS Error: {0}")]
    JSError(#[from] JSError),
    #[error("Recv Error: {0}")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Send Error")]
    SendError,
    #[error("KaTeX Error: math: {latex}, macros: {macros:?}, error: {message}")]
    KaTeXError { message: String, latex: String, macros: BTreeMap<String, String> },
}

pub fn set_cache(path: impl AsRef<Path>) {
    init_katex_worker(Some(path.as_ref().to_path_buf()));
}

pub(crate) trait Core: Sized {
    type Error;
    // スナップショットを採れなかったとき
    fn new() -> Result<Self, Self::Error>;
    // snapshotを取り出す/または作成してからランタイムを返す
    fn new_with_snapshot(path: &Path) -> Result<Self, Self::Error>;
    fn exec(&mut self, input: Input) -> Result<Output, Self::Error>;
}

fn init_katex_worker(cache: Option<PathBuf>) {
    if KATEX_WORKER.get().is_some() {
        return;
    }
    let (tx, rx): (Sender<(Input, Sender<Result<Output, Error>>)>, Receiver<(Input, Sender<Result<Output, Error>>)>) = mpsc::channel();
    thread::spawn(move || {
        let mut runtime =
            if let Some(cache) = cache { <Engine as Core>::new_with_snapshot(&cache).unwrap() } else { <Engine as Core>::new().unwrap() };
        for (katex_input, sender) in rx {
            let res = runtime.exec(katex_input).map_err(Error::from);
            sender.send(res).unwrap();
        }
    });
    KATEX_WORKER.set(KatexWorker(tx)).ok().unwrap();
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

    worker
        .0
        .send((Input { latex: latex.to_string(), options: options.clone(), macros: macros.clone() }, tx))
        .map_err(|_| Error::SendError)?;
    // let out_str = ;
    match rx.recv()?? {
        Output::Success { html, macros: macros_value } => {
            *macros = macros_value;
            Ok(html)
        }
        Output::Error { error, macros: macros_value } => {
            Err(Error::KaTeXError { message: error, latex: latex.to_string(), macros: macros_value })
        }
    }
}

pub use font::{UsedFonts, font_extract};
