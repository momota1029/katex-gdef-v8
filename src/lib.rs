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
katex-gdef-v8 = "0.1.3"
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
let font_flags = font_extract(&html);

// Check if specific fonts are used
println!("Is empty: {}", font_flags.is_empty());

// Iterate through used fonts
for font_name in font_flags.clone() {
    // Each font_name is the base name (e.g., "KaTeX_Math-Italic")
    // To get the complete font file name, add file extension:
    println!("Font file: {}.woff2", font_name);
}

// Collect all font names into a HashSet
let font_set: HashSet<&str> = font_flags.collect();

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

use deno_core::{JsRuntime, RuntimeOptions, error::CoreError, v8};
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
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
struct Input<'a> {
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
enum Output {
    Success { html: String, macros: BTreeMap<String, String> },
    Error { error: String, macros: BTreeMap<String, String> },
}

struct KatexWorker(Sender<(String, Sender<Result<String, Error>>)>);
static KATEX_WORKER: OnceCell<KatexWorker> = OnceCell::new();

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

pub fn font_extract(html: &str) -> UsedFonts {
    let mut flags = UsedFonts::default();
    let mut tokenizer = Tokenizer::new(
        FontSink {
            font_stack: vec![Font { family: FontFamilies::Main, bold: false, italic: false, delimisizing_mult: false }],
            font_flags: &mut flags,
        },
        TokenizerOpts::default(),
    );
    let mut queue = BufferQueue::default();
    queue.push_back(html.into());

    let _ = tokenizer.feed(&mut queue);
    tokenizer.end();
    flags
}

struct FontSink<'a> {
    font_stack: Vec<Font>,
    font_flags: &'a mut UsedFonts,
}
impl TokenSink for FontSink<'_> {
    type Handle = ();
    fn process_token(&mut self, token: Token, _: u64) -> TokenSinkResult<Self::Handle> {
        match token {
            Token::TagToken(tag) => match tag.kind {
                html5ever::tokenizer::TagKind::EndTag if &tag.name.to_ascii_lowercase() == "span" => {
                    self.font_stack.pop();
                }
                html5ever::tokenizer::TagKind::StartTag if &tag.name.to_ascii_lowercase() == "span" => {
                    let mut last = *self.font_stack.last().unwrap();
                    for attr in &tag.attrs {
                        if &attr.name.local.to_ascii_lowercase() == "class" {
                            let (mut delimsizing, mut mult, mut op_symbol) = (false, false, false);
                            for class in attr.value.split_whitespace() {
                                delimsizing |= class == "delimsizing";
                                mult |= class == "mult";
                                op_symbol |= class == "op-symbol";
                            }
                            last.delimisizing_mult |= delimsizing && mult;
                            for class in attr.value.split_whitespace() {
                                font_stack_set(&mut last, class, delimsizing, op_symbol);
                            }
                        }
                    }
                    self.font_stack.push(last);
                }
                _ => (),
            },
            Token::CharacterTokens(tendril) if !tendril.trim().is_empty() => {
                font_flag_set(*self.font_stack.last().unwrap(), &mut self.font_flags)
            }
            _ => (),
        }
        TokenSinkResult::Continue
    }
}

#[derive(Debug, Clone, Copy)]
struct Font {
    family: FontFamilies,
    bold: bool,
    italic: bool,
    delimisizing_mult: bool, // has span.delimsizing.mult as parent
}
#[derive(Debug, Clone, Copy)]
enum FontFamilies {
    AMS,
    Caligraphic,
    Fraktur,
    Main,
    Math,
    SansSerif,
    Script,
    Size1,
    Size2,
    Size3,
    Size4,
    Typewriter,
}
#[inline(always)]
fn font_stack_set(font: &mut Font, class: &str, delimisizing: bool, op_symbol: bool) {
    match class {
        "textbf" => font.bold = true,
        "textit" => font.italic = true,
        "textrm" => font.family = FontFamilies::Main,
        "mathsf" | "textsf" => font.family = FontFamilies::SansSerif,
        "texttt" => font.family = FontFamilies::Typewriter,
        "mathnormal" => {
            font.family = FontFamilies::Math;
            font.italic = true
        }
        "mathit" => {
            font.family = FontFamilies::Main;
            font.italic = true
        }
        "mathrm" => font.italic = false,
        "mathbf" => {
            font.family = FontFamilies::Main;
            font.bold = true
        }
        "boldsymbol" => {
            font.family = FontFamilies::Math;
            font.bold = true;
            font.italic = true
        }
        "amsrm" | "mathbb" | "textbb" => font.family = FontFamilies::AMS,
        "mathcal" => font.family = FontFamilies::Caligraphic,
        "mathfrak" | "textfrak" => font.family = FontFamilies::Fraktur,
        "mathboldfrak" | "textboldfrak" => {
            font.family = FontFamilies::Fraktur;
            font.bold = true
        }
        "mathtt" => font.family = FontFamilies::Typewriter,
        "mathscr" => font.family = FontFamilies::Script,
        "mathboldsf" | "textboldsf" => {
            font.family = FontFamilies::SansSerif;
            font.bold = true
        }
        "mathsfit" | "mathitsf" | "textitsf" => {
            font.family = FontFamilies::SansSerif;
            font.italic = true
        }
        "mainrm" => {
            font.family = FontFamilies::Main;
            font.italic = false
        }
        // "mult" => font.delimisizing_mult = true,
        "size1" if delimisizing => font.family = FontFamilies::Size1,
        "size2" if delimisizing => font.family = FontFamilies::Size2,
        "size3" if delimisizing => font.family = FontFamilies::Size3,
        "size4" if delimisizing => font.family = FontFamilies::Size4,
        "delim-size1" if font.delimisizing_mult => font.family = FontFamilies::Size1,
        "delim-size4" if font.delimisizing_mult => font.family = FontFamilies::Size4,
        "small-op" if op_symbol => font.family = FontFamilies::Size1,
        "large-op" if op_symbol => font.family = FontFamilies::Size2,
        _ => (),
    }
}

#[derive(Debug, Clone, Copy, Hash, Deserialize, Serialize)]
pub struct UsedFonts {
    katex_ams_regular: bool,
    katex_caligraphic_bold: bool,
    katex_caligraphic_regular: bool,
    katex_fraktur_bold: bool,
    katex_fraktur_regular: bool,
    katex_main_bold: bool,
    katex_main_bolditalic: bool,
    katex_main_italic: bool,
    katex_main_regular: bool,
    katex_math_bolditalic: bool,
    katex_math_italic: bool,
    katex_sansserif_bold: bool,
    katex_sansserif_italic: bool,
    katex_sansserif_regular: bool,
    katex_script_regular: bool,
    katex_size1_regular: bool,
    katex_size2_regular: bool,
    katex_size3_regular: bool,
    katex_size4_regular: bool,
    katex_typewriter_regular: bool,
}
impl Default for UsedFonts {
    fn default() -> Self {
        UsedFonts {
            katex_ams_regular: false,
            katex_caligraphic_bold: false,
            katex_caligraphic_regular: false,
            katex_fraktur_bold: false,
            katex_fraktur_regular: false,
            katex_main_bold: false,
            katex_main_bolditalic: false,
            katex_main_italic: false,
            katex_main_regular: false,
            katex_math_bolditalic: false,
            katex_math_italic: false,
            katex_sansserif_bold: false,
            katex_sansserif_italic: false,
            katex_sansserif_regular: false,
            katex_script_regular: false,
            katex_size1_regular: false,
            katex_size2_regular: false,
            katex_size3_regular: false,
            katex_size4_regular: false,
            katex_typewriter_regular: false,
        }
    }
}
impl UsedFonts {
    pub fn is_empty(&self) -> bool {
        !self.katex_ams_regular
            && !self.katex_caligraphic_bold
            && !self.katex_caligraphic_regular
            && !self.katex_fraktur_bold
            && !self.katex_fraktur_regular
            && !self.katex_main_bold
            && !self.katex_main_bolditalic
            && !self.katex_main_italic
            && !self.katex_main_regular
            && !self.katex_math_bolditalic
            && !self.katex_math_italic
            && !self.katex_sansserif_bold
            && !self.katex_sansserif_italic
            && !self.katex_sansserif_regular
            && !self.katex_script_regular
            && !self.katex_size1_regular
            && !self.katex_size2_regular
            && !self.katex_size3_regular
            && !self.katex_size4_regular
            && !self.katex_typewriter_regular
    }
    pub fn merge(&mut self, other: UsedFonts) {
        self.katex_ams_regular |= other.katex_ams_regular;
        self.katex_caligraphic_bold |= other.katex_caligraphic_bold;
        self.katex_caligraphic_regular |= other.katex_caligraphic_regular;
        self.katex_fraktur_bold |= other.katex_fraktur_bold;
        self.katex_fraktur_regular |= other.katex_fraktur_regular;
        self.katex_main_bold |= other.katex_main_bold;
        self.katex_main_bolditalic |= other.katex_main_bolditalic;
        self.katex_main_italic |= other.katex_main_italic;
        self.katex_main_regular |= other.katex_main_regular;
        self.katex_math_bolditalic |= other.katex_math_bolditalic;
        self.katex_math_italic |= other.katex_math_italic;
        self.katex_sansserif_bold |= other.katex_sansserif_bold;
        self.katex_sansserif_italic |= other.katex_sansserif_italic;
        self.katex_sansserif_regular |= other.katex_sansserif_regular;
        self.katex_script_regular |= other.katex_script_regular;
        self.katex_size1_regular |= other.katex_size1_regular;
        self.katex_size2_regular |= other.katex_size2_regular;
        self.katex_size3_regular |= other.katex_size3_regular;
        self.katex_size4_regular |= other.katex_size4_regular;
        self.katex_typewriter_regular |= other.katex_typewriter_regular;
    }
}
impl Iterator for UsedFonts {
    type Item = &'static str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.katex_ams_regular {
            self.katex_ams_regular = false;
            return Some("KaTeX_AMS-Regular");
        }
        if self.katex_caligraphic_bold {
            self.katex_caligraphic_bold = false;
            return Some("KaTeX_Caligraphic-Bold");
        }
        if self.katex_caligraphic_regular {
            self.katex_caligraphic_regular = false;
            return Some("KaTeX_Caligraphic-Regular");
        }
        if self.katex_fraktur_bold {
            self.katex_fraktur_bold = false;
            return Some("KaTeX_Fraktur-Bold");
        }
        if self.katex_fraktur_regular {
            self.katex_fraktur_regular = false;
            return Some("KaTeX_Fraktur-Regular");
        }
        if self.katex_main_bold {
            self.katex_main_bold = false;
            return Some("KaTeX_Main-Bold");
        }
        if self.katex_main_bolditalic {
            self.katex_main_bolditalic = false;
            return Some("KaTeX_Main-BoldItalic");
        }
        if self.katex_main_italic {
            self.katex_main_italic = false;
            return Some("KaTeX_Main-Italic");
        }
        if self.katex_main_regular {
            self.katex_main_regular = false;
            return Some("KaTeX_Main-Regular");
        }
        if self.katex_math_bolditalic {
            self.katex_math_bolditalic = false;
            return Some("KaTeX_Math-BoldItalic");
        }
        if self.katex_math_italic {
            self.katex_math_italic = false;
            return Some("KaTeX_Math-Italic");
        }
        if self.katex_sansserif_bold {
            self.katex_sansserif_bold = false;
            return Some("KaTeX_SansSerif-Bold");
        }
        if self.katex_sansserif_italic {
            self.katex_sansserif_italic = false;
            return Some("KaTeX_SansSerif-Italic");
        }
        if self.katex_sansserif_regular {
            self.katex_sansserif_regular = false;
            return Some("KaTeX_SansSerif-Regular");
        }
        if self.katex_script_regular {
            self.katex_script_regular = false;
            return Some("KaTeX_Script-Regular");
        }
        if self.katex_size1_regular {
            self.katex_size1_regular = false;
            return Some("KaTeX_Size1-Regular");
        }
        if self.katex_size2_regular {
            self.katex_size2_regular = false;
            return Some("KaTeX_Size2-Regular");
        }
        if self.katex_size3_regular {
            self.katex_size3_regular = false;
            return Some("KaTeX_Size3-Regular");
        }
        if self.katex_size4_regular {
            self.katex_size4_regular = false;
            return Some("KaTeX_Size4-Regular");
        }
        if self.katex_typewriter_regular {
            self.katex_typewriter_regular = false;
            return Some("KaTeX_Typewriter-Regular");
        }
        None
    }
}
#[inline(always)]
fn font_flag_set(font: Font, flags: &mut UsedFonts) {
    match font.family {
        FontFamilies::AMS => flags.katex_ams_regular = true,
        FontFamilies::Caligraphic if font.bold => flags.katex_caligraphic_bold = true,
        FontFamilies::Caligraphic => flags.katex_caligraphic_regular = true,
        FontFamilies::Fraktur if font.bold => flags.katex_fraktur_bold = true,
        FontFamilies::Fraktur => flags.katex_fraktur_regular = true,
        FontFamilies::Main => match (font.bold, font.italic) {
            (false, false) => flags.katex_main_regular = true,
            (true, false) => flags.katex_main_bold = true,
            (false, true) => flags.katex_main_italic = true,
            (true, true) => flags.katex_main_bolditalic = true,
        },
        FontFamilies::Math if font.bold => flags.katex_math_bolditalic = true,
        FontFamilies::Math => flags.katex_math_italic = true,
        FontFamilies::SansSerif => match (font.bold, font.italic) {
            (false, false) => flags.katex_sansserif_regular = true,
            (true, false) => flags.katex_sansserif_bold = true,
            (false, true) => flags.katex_sansserif_italic = true,
            (true, true) => {
                flags.katex_sansserif_bold = true;
                flags.katex_sansserif_italic = true
            }
        },
        FontFamilies::Script => flags.katex_script_regular = true,
        FontFamilies::Size1 => flags.katex_size1_regular = true,
        FontFamilies::Size2 => flags.katex_size2_regular = true,
        FontFamilies::Size3 => flags.katex_size3_regular = true,
        FontFamilies::Size4 => flags.katex_size4_regular = true,
        FontFamilies::Typewriter => flags.katex_typewriter_regular = true,
    }
}
