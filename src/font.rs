use serde::{Deserialize, Serialize};

#[inline(always)]
pub fn font_extract(html: &str) -> UsedFonts {
    let mut fonts = UsedFonts::default();
    let mut tokenizer = html5gum::Tokenizer::new(html);
    while let Some(Ok(token)) = tokenizer.next() {
        let html5gum::Token::StartTag(tag) = token else { continue };
        if tag.name.to_ascii_lowercase() != b"span" {
            continue;
        }
        let Some(Ok(class_list)) = tag.attributes.get(b"class".as_slice()).map(|s| std::str::from_utf8(&s)) else { continue };
        if class_list.split_whitespace().any(|class| class == "katex-html") {
            calc_font_property(Font::default(), &mut fonts, &mut tokenizer);
            break;
        }
    }
    fonts
}

// 開始タグ直後から終了タグ終わりまで読む関数
#[inline]
fn calc_font_property(font: Font, font_flags: &mut UsedFonts, tokens: &mut html5gum::Tokenizer<html5gum::StringReader>) {
    while let Some(Ok(token)) = tokens.next() {
        match token {
            html5gum::Token::EndTag(tag) if tag.name.to_ascii_lowercase() == b"span" => return,
            html5gum::Token::String(s) if !s.trim_ascii().is_empty() => font_flag_set(font, font_flags),
            html5gum::Token::StartTag(tag) if tag.name.to_ascii_lowercase() == b"span" => {
                let mut child_font = font;
                if let Some(Ok(class_list)) = tag.attributes.get(b"class".as_slice()).map(|s| std::str::from_utf8(&s)) {
                    let (mut delimsizing, mut mult, mut op_symbol) = (false, false, false);
                    for class in class_list.split_whitespace() {
                        match class {
                            "delimsizing" => delimsizing = true,
                            "mult" => mult = true,
                            "op-symbol" => op_symbol = true,
                            _ => (),
                        }
                    }
                    child_font.delimisizing_mult |= delimsizing && mult;
                    for class in class_list.split_whitespace() {
                        font_stack_set(&mut child_font, class, delimsizing, op_symbol);
                    }
                    calc_font_property(child_font, font_flags, tokens);
                }
            }
            _ => (),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Font {
    family: FontFamilies,
    bold: bool,
    italic: bool,
    delimisizing_mult: bool, // has span.delimsizing.mult as parent
}
#[derive(Debug, Clone, Copy, Default)]
enum FontFamilies {
    AMS,
    Caligraphic,
    Fraktur,
    #[default]
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
