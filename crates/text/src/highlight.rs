//! Code-block highlighting: a small table-driven tokenizer per language.
//!
//! Lighter than a grammar engine (no data to load, no regex per language)
//! and enough for reading: keywords, strings, comments, numbers, names. It
//! knows every language `detect` can return, plus their common aliases.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

/// highlight.js classes. Colours, from `markdown.css`:
/// `--hl-comment` Comment Quote · `--hl-keyword` Keyword Literal Section Name Meta ·
/// `--hl-string` String Attribute · `--hl-number` Number Symbol Bullet Link ·
/// `--hl-func` Title · `--hl-type` Type BuiltIn · `--hl-attr` Attr Variable ·
/// Emphasis italic, Strong bold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Comment,
    Quote,
    Keyword,
    Literal,
    Section,
    /// Tag name in HTML/XML.
    Name,
    /// Preprocessor lines, decorators, `<?php`.
    Meta,
    String,
    /// CSS property name.
    Attribute,
    Number,
    /// CSS pseudo-classes and attribute selectors.
    Symbol,
    Bullet,
    Link,
    /// Function and class names, CSS class and id selectors.
    Title,
    Type,
    BuiltIn,
    /// HTML attributes, JSON/YAML/TOML keys.
    Attr,
    Variable,
    Emphasis,
    Strong,
}

pub type Spans = Vec<(Range<usize>, Token)>;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Code,
    Markup,
    Css,
    Config,
    Markdown,
}

#[derive(Clone, Copy)]
struct Lang {
    /// Canonical name first, then aliases.
    names: &'static [&'static str],
    mode: Mode,
    keywords: &'static str,
    literals: &'static str,
    types: &'static str,
    builtins: &'static str,
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static str,
    /// Keywords match regardless of case (SQL, PowerShell, Dockerfile).
    caseless: bool,
    dollar_vars: bool,
    /// `@decorator` / `@Annotation`.
    at_meta: bool,
    /// A `#` line is a preprocessor or attribute line (C, C#, Rust).
    hash_meta: bool,
    /// A capitalised identifier names a type or class.
    cap_types: bool,
    triple_quotes: bool,
    /// A string followed by `:` is a key (JSON).
    key_strings: bool,
    /// `Verb-Noun` identifiers (PowerShell cmdlets).
    dash_idents: bool,
    meta_marks: &'static [&'static str],
}

const BASE: Lang = Lang {
    names: &[],
    mode: Mode::Code,
    keywords: "",
    literals: "true false null",
    types: "",
    builtins: "",
    line: &["//"],
    block: Some(("/*", "*/")),
    quotes: "\"'",
    caseless: false,
    dollar_vars: false,
    at_meta: false,
    hash_meta: false,
    cap_types: false,
    triple_quotes: false,
    key_strings: false,
    dash_idents: false,
    meta_marks: &[],
};

const JS_KEYWORDS: &str = "as async await break case catch class const continue debugger default delete do else export extends finally for from function get if import in instanceof let new of return set static super switch this throw try typeof var void while with yield";
const JS_BUILTINS: &str = "console window document process require module exports globalThis Math JSON Object Array String Number Boolean Promise Map Set Symbol Error RegExp Date parseInt parseFloat setTimeout setInterval fetch";
const TS_KEYWORDS: &str = "as async await break case catch class const continue debugger default delete do else export extends finally for from function get if import in instanceof let new of return set static super switch this throw try typeof var void while with yield interface type enum implements namespace declare abstract private protected public readonly keyof infer is satisfies override";
const C_TYPES: &str = "bool char double float int long short signed unsigned void size_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t";
const CONFIG: Lang = Lang { mode: Mode::Config, line: &["#"], block: None, literals: "true false null", ..BASE };

const LANGS: &[Lang] = &[
    Lang {
        names: &["python", "py", "python3", "gyp"],
        keywords: "and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case",
        literals: "True False None",
        builtins: "print len range open str int float list dict set tuple bool type isinstance super self cls enumerate zip map filter sorted min max sum abs any all input repr object Exception",
        line: &["#"],
        block: None,
        at_meta: true,
        triple_quotes: true,
        ..BASE
    },
    Lang {
        names: &["typescript", "ts", "tsx", "mts", "cts"],
        keywords: TS_KEYWORDS,
        literals: "true false null undefined NaN Infinity",
        types: "string number boolean any unknown never void object bigint symbol",
        builtins: JS_BUILTINS,
        quotes: "\"'`",
        at_meta: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["javascript", "js", "mjs", "cjs", "node"],
        keywords: JS_KEYWORDS,
        literals: "true false null undefined NaN Infinity",
        builtins: JS_BUILTINS,
        quotes: "\"'`",
        ..BASE
    },
    Lang {
        names: &["jsx"],
        keywords: JS_KEYWORDS,
        literals: "true false null undefined NaN Infinity",
        builtins: JS_BUILTINS,
        quotes: "\"'`",
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["java", "jsp"],
        keywords: "abstract assert break case catch class continue default do else enum extends final finally for if implements import instanceof interface native new package private protected public return static strictfp super switch synchronized this throw throws transient try volatile while var record yield sealed permits",
        types: "boolean byte char double float int long short void String",
        builtins: "System Math",
        at_meta: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["csharp", "cs", "c#"],
        keywords: "abstract as async await base break case catch checked class const continue default delegate do else enum event explicit extern finally fixed for foreach goto if implicit in interface internal is lock namespace new operator out override params private protected public readonly record ref return sealed sizeof stackalloc static struct switch this throw try typeof unchecked unsafe using var virtual volatile when where while yield get set init value",
        types: "bool byte char decimal double dynamic float int long object sbyte short string uint ulong ushort void",
        builtins: "Console Math String",
        hash_meta: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["cpp", "c++", "cc", "cxx", "hpp", "hh", "hxx"],
        keywords: "alignas alignof auto break case catch class const constexpr consteval constinit const_cast continue co_await co_return co_yield decltype default delete do dynamic_cast else enum explicit export extern for friend goto if inline mutable namespace new noexcept operator private protected public register reinterpret_cast requires return sizeof static static_assert static_cast struct switch template this thread_local throw try typedef typeid typename union using virtual volatile while concept override final",
        literals: "true false nullptr NULL",
        types: C_TYPES,
        builtins: "std string vector map unordered_map set cout cin cerr endl printf malloc free",
        hash_meta: true,
        ..BASE
    },
    Lang {
        names: &["c", "h"],
        keywords: "auto break case const continue default do else enum extern for goto if inline register restrict return sizeof static struct switch typedef union volatile while",
        literals: "true false NULL",
        types: C_TYPES,
        builtins: "printf scanf malloc calloc realloc free memcpy memset strlen strcmp strcpy fopen fclose fprintf sprintf puts",
        hash_meta: true,
        ..BASE
    },
    Lang {
        names: &["php"],
        keywords: "abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require require_once return static switch throw trait try unset use var while xor yield",
        literals: "true false null TRUE FALSE NULL",
        line: &["//", "#"],
        dollar_vars: true,
        meta_marks: &["<?php", "<?=", "?>"],
        ..BASE
    },
    Lang {
        names: &["ruby", "rb", "gemspec", "podspec", "rake"],
        keywords: "alias and begin break case class def defined do else elsif end ensure for if in module next not or redo rescue retry return self super then undef unless until when while yield require require_relative include extend attr_accessor attr_reader attr_writer private protected public lambda proc",
        literals: "true false nil",
        builtins: "puts print p raise",
        line: &["#"],
        block: None,
        quotes: "\"'`",
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["go", "golang"],
        keywords: "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var",
        literals: "true false nil iota",
        types: "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any",
        builtins: "append cap close complex copy delete imag len make new panic print println real recover",
        quotes: "\"'`",
        ..BASE
    },
    Lang {
        names: &["rust", "rs"],
        keywords: "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return static struct super trait type unsafe use where while self Self",
        literals: "true false None Some Ok Err",
        types: "bool char f32 f64 i8 i16 i32 i64 i128 isize str u8 u16 u32 u64 u128 usize String Vec Option Result Box",
        builtins: "println print eprintln format vec panic assert assert_eq matches write writeln todo unimplemented",
        // `'` opens a lifetime more often than a char; chars are matched on their own.
        quotes: "\"",
        hash_meta: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["kotlin", "kt", "kts"],
        keywords: "abstract actual annotation as break by catch class companion const constructor continue crossinline data do else enum expect external final finally for fun get if import in infix init inline inner interface internal is lateinit noinline object open operator out override package private protected public reified return sealed set super suspend tailrec this throw try typealias val var vararg when where while",
        types: "Int Long Short Byte Float Double Boolean Char String Unit Any Nothing",
        at_meta: true,
        cap_types: true,
        triple_quotes: true,
        ..BASE
    },
    Lang {
        names: &["swift"],
        keywords: "associatedtype async await break case catch class continue default defer deinit do else enum extension fallthrough fileprivate for func guard if import in init inout internal is let open operator private protocol public repeat rethrows return self Self static struct subscript super switch throw throws try typealias var where while some any",
        literals: "true false nil",
        types: "Int Double Float String Bool Character Void Array Dictionary Set Optional",
        at_meta: true,
        hash_meta: true,
        cap_types: true,
        triple_quotes: true,
        ..BASE
    },
    Lang {
        names: &["sql", "mysql", "postgres", "postgresql", "pgsql", "sqlite", "plsql", "tsql"],
        keywords: "select from where and or not insert into values update set delete create table index view drop alter add column primary key foreign references join inner left right outer full cross on as group by order having limit offset union all distinct case when then else end exists in is like between with returning default constraint unique check if begin commit rollback transaction grant revoke asc desc",
        literals: "null true false",
        types: "int integer bigint smallint tinyint decimal numeric float real double varchar char text date time timestamp datetime boolean bool blob json uuid serial",
        builtins: "count sum avg min max coalesce now cast concat substring lower upper length round",
        line: &["--"],
        quotes: "'\"`",
        caseless: true,
        ..BASE
    },
    Lang {
        names: &["bash", "sh", "shell", "zsh", "console", "shellsession"],
        keywords: "if then else elif fi for while until do done case esac in function select time return break continue local export declare readonly unset shift",
        literals: "true false",
        builtins: "echo printf read cd pwd source exit eval exec set test trap alias let type",
        line: &["#"],
        block: None,
        dollar_vars: true,
        ..BASE
    },
    Lang {
        names: &["powershell", "ps", "ps1", "pwsh"],
        keywords: "begin break catch class continue data do dynamicparam else elseif end enum exit filter finally for foreach function if in param process return switch throw trap try until using while workflow",
        literals: "",
        line: &["#"],
        block: Some(("<#", "#>")),
        caseless: true,
        dollar_vars: true,
        dash_idents: true,
        ..BASE
    },
    Lang { names: &["css"], mode: Mode::Css, ..BASE },
    Lang { names: &["scss", "sass", "less"], mode: Mode::Css, dollar_vars: true, ..BASE },
    Lang { names: &["html", "htm", "xhtml", "vue", "svelte"], mode: Mode::Markup, ..BASE },
    Lang { names: &["xml", "svg", "rss", "atom", "plist", "xsd", "xsl", "wsf"], mode: Mode::Markup, ..BASE },
    Lang { names: &["yaml", "yml"], literals: "true false null yes no on off ~", ..CONFIG },
    Lang { names: &["toml"], ..CONFIG },
    Lang { names: &["ini", "cfg", "conf", "properties", "editorconfig", "dotenv", "env"], line: &[";", "#"], ..CONFIG },
    Lang { names: &["json", "jsonc", "json5", "geojson"], quotes: "\"", key_strings: true, ..BASE },
    Lang { names: &["markdown", "md", "mkd", "mkdown"], mode: Mode::Markdown, ..BASE },
    Lang {
        names: &["lua", "luau"],
        keywords: "and break do else elseif end for function goto if in local not or repeat return then until while",
        literals: "true false nil",
        builtins: "print pairs ipairs require type tostring tonumber table string math os io error assert pcall setmetatable getmetatable select next",
        line: &["--"],
        block: Some(("--[[", "]]")),
        ..BASE
    },
    Lang {
        names: &["r"],
        keywords: "if else repeat while function for in next break return",
        literals: "TRUE FALSE NULL NA Inf NaN T F",
        builtins: "library require c list print cat paste paste0 length seq rep sum mean",
        line: &["#"],
        block: None,
        ..BASE
    },
    Lang {
        names: &["dart"],
        keywords: "abstract as assert async await break case catch class const continue covariant default deferred do dynamic else enum export extends extension external factory final finally for get if implements import in interface is late library mixin new on operator part required rethrow return sealed set show static super switch sync this throw try typedef var void while with yield",
        types: "int double num String bool List Map Set Future Stream Object Widget",
        at_meta: true,
        cap_types: true,
        triple_quotes: true,
        ..BASE
    },
    Lang {
        names: &["haskell", "hs"],
        keywords: "case class data default deriving do else if import in infix infixl infixr instance let module newtype of then type where qualified as hiding forall",
        literals: "True False",
        line: &["--"],
        block: Some(("{-", "-}")),
        quotes: "\"",
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["graphql", "gql"],
        keywords: "query mutation subscription fragment on type input enum interface union scalar schema extend implements directive",
        line: &["#"],
        block: None,
        quotes: "\"",
        dollar_vars: true,
        at_meta: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        names: &["elixir", "ex", "exs"],
        keywords: "after alias and case catch cond def defp defmacro defmacrop defmodule defprotocol defimpl defstruct do else end fn for if import in not or quote raise receive require rescue try unless unquote use when with",
        literals: "true false nil",
        line: &["#"],
        block: None,
        at_meta: true,
        cap_types: true,
        triple_quotes: true,
        ..BASE
    },
    Lang {
        names: &["dockerfile", "docker"],
        keywords: "from as run cmd label maintainer expose env add copy entrypoint volume user workdir arg onbuild stopsignal healthcheck shell",
        literals: "",
        line: &["#"],
        block: None,
        caseless: true,
        dollar_vars: true,
        ..BASE
    },
    Lang {
        names: &["nginx", "nginxconf"],
        keywords: "server location listen server_name root index proxy_pass proxy_set_header return rewrite upstream include error_page access_log error_log try_files events http worker_processes worker_connections ssl_certificate ssl_certificate_key gzip add_header if set",
        literals: "on off",
        line: &["#"],
        block: None,
        dollar_vars: true,
        ..BASE
    },
];

/// Names a definition keyword gives to the identifier after it.
const DEFINERS: &[&str] = &[
    "def", "fn", "func", "function", "fun", "class", "struct", "enum", "interface", "trait", "type", "impl", "module",
    "defmodule", "defp", "record", "union", "protocol", "extension", "object",
];

/// Canonical language names, for the language picker.
pub fn languages() -> impl Iterator<Item = &'static str> {
    LANGS.iter().map(|l| l.names[0])
}

/// Highlight `code` written in `lang` (a fence info word or a `detect` result).
/// Unknown languages get no spans. Ranges are byte offsets, in order.
pub fn highlight(lang: &str, code: &str) -> Spans {
    let lang = lang.trim().to_ascii_lowercase();
    let Some(index) = LANGS.iter().position(|l| l.names.contains(&lang.as_str())) else { return Vec::new() };
    let l = &LANGS[index];
    let mut out = Vec::new();
    match l.mode {
        Mode::Code => code_spans(l, &WORDS[index], code, 0, &mut out),
        Mode::Markup => markup(code, &mut out),
        Mode::Css => css(l, code, &mut out),
        Mode::Config => config(l, &WORDS[index], code, &mut out),
        Mode::Markdown => markdown(code, &mut out),
    }
    out
}

static WORDS: LazyLock<Vec<HashMap<&'static str, Token>>> = LazyLock::new(|| {
    LANGS
        .iter()
        .map(|l| {
            let mut map = HashMap::new();
            for (list, token) in
                [(l.builtins, Token::BuiltIn), (l.types, Token::Type), (l.literals, Token::Literal), (l.keywords, Token::Keyword)]
            {
                map.extend(list.split_whitespace().map(|w| (w, token)));
            }
            map
        })
        .collect()
});

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn line_end(s: &str, i: usize) -> usize {
    s[i..].find('\n').map_or(s.len(), |n| i + n)
}

fn char_len(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(1, char::len_utf8)
}

/// End of a string opened at `i` by `q`. `"` and `'` stop at a newline.
fn string_end(b: &[u8], i: usize, q: u8) -> usize {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            c if c == q => return j + 1,
            b'\n' if q != b'`' => return j,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// A number starting at `i`: hex, digits with `_`, a fraction, an exponent and
/// any suffix or unit (`u32`, `px`, `%`).
fn number_end(b: &[u8], i: usize) -> usize {
    let mut j = i;
    let digit = |c: u8| c.is_ascii_hexdigit() || c == b'_';
    if b[j] == b'0' && b.get(j + 1).is_some_and(|c| matches!(c, b'x' | b'X' | b'b' | b'B' | b'o' | b'O')) {
        j += 2;
        while j < b.len() && digit(b[j]) {
            j += 1;
        }
    } else {
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
            j += 1;
        }
        if b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        if matches!(b.get(j), Some(b'e' | b'E')) && b.get(j + 1).is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+') {
            j += 2;
        }
    }
    while j < b.len() && (ident_char(b[j]) || b[j] == b'%') {
        j += 1;
    }
    j
}

fn code_spans(l: &Lang, words: &HashMap<&str, Token>, s: &str, base: usize, out: &mut Spans) {
    let b = s.as_bytes();
    let mut push = |r: Range<usize>, t| out.push((r.start + base..r.end + base, t));
    let mut i = 0;
    let mut line_start = true;
    let mut prev = "";
    while i < b.len() {
        let c = b[i];
        let rest = &s[i..];
        if c == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let was_line_start = std::mem::replace(&mut line_start, false);
        let mut token = |end: usize, t: Token| {
            push(i..end, t);
            end
        };
        if l.hash_meta && was_line_start && c == b'#' {
            i = token(line_end(s, i), Token::Meta);
        } else if let Some(m) = l.meta_marks.iter().find(|m| rest.starts_with(**m)) {
            i = token(i + m.len(), Token::Meta);
        } else if let Some((open, close)) = l.block.filter(|(open, _)| rest.starts_with(open)) {
            let end = rest[open.len()..].find(close).map_or(s.len(), |n| i + open.len() + n + close.len());
            i = token(end, Token::Comment);
        } else if l.line.iter().any(|m| rest.starts_with(m)) {
            i = token(line_end(s, i), Token::Comment);
        } else if l.triple_quotes && (rest.starts_with("\"\"\"") || rest.starts_with("'''")) {
            let end = rest[3..].find(&rest[..3]).map_or(s.len(), |n| i + 6 + n);
            i = token(end, Token::String);
        } else if l.quotes.as_bytes().contains(&c) {
            let end = string_end(b, i, c);
            let key = l.key_strings && s[end..].trim_start().starts_with(':');
            i = token(end, if key { Token::Attr } else { Token::String });
        } else if c == b'\'' && char_literal(rest) > 0 {
            i = token(i + char_literal(rest), Token::String);
        } else if c.is_ascii_digit() && (i == 0 || !ident_char(b[i - 1])) {
            i = token(number_end(b, i), Token::Number);
        } else if l.dollar_vars && c == b'$' && i + 1 < b.len() {
            let n = b[i + 1];
            let end = if n == b'{' {
                rest.find('}').map_or(s.len(), |e| i + e + 1)
            } else if ident_start(n) {
                i + 1 + rest[1..].bytes().take_while(|c| ident_char(*c)).count()
            } else if b"#?@*!$-".contains(&n) || n.is_ascii_digit() {
                i + 2
            } else {
                i + 1
            };
            if end > i + 1 {
                i = token(end, Token::Variable);
            } else {
                i += 1;
            }
        } else if l.at_meta && c == b'@' && b.get(i + 1).is_some_and(|c| ident_start(*c)) {
            let end = i + 1 + rest[1..].bytes().take_while(|c| ident_char(*c) || *c == b'.').count();
            i = token(end, Token::Meta);
        } else if ident_start(c) {
            let mut end = i + rest.bytes().take_while(|c| ident_char(*c)).count();
            while l.dash_idents && b.get(end) == Some(&b'-') && b.get(end + 1).is_some_and(u8::is_ascii_alphabetic) {
                end += 1 + s[end + 1..].bytes().take_while(|c| ident_char(*c)).count();
            }
            let word = &s[i..end];
            let lower;
            let key = if l.caseless {
                lower = word.to_ascii_lowercase();
                lower.as_str()
            } else {
                word
            };
            let kind = match words.get(key) {
                Some(t) => Some(*t),
                None if word.contains('-') => Some(Token::BuiltIn),
                None if DEFINERS.contains(&prev) => Some(Token::Title),
                None if s[end..].trim_start_matches([' ', '\t']).starts_with('(') => Some(Token::Title),
                None if l.cap_types && c.is_ascii_uppercase() => Some(Token::Type),
                None => None,
            };
            if let Some(t) = kind {
                push(i..end, t);
            }
            prev = word;
            i = end;
            continue;
        } else {
            i += char_len(s, i);
        }
        prev = "";
    }
}

/// Length of a char literal (`'a'`, `'\n'`, `'中'`) at the start of `s`, or 0.
fn char_literal(s: &str) -> usize {
    let mut chars = s[1..].char_indices();
    match chars.next() {
        Some((_, '\\')) => s[2..].find('\'').filter(|n| *n <= 8).map_or(0, |n| n + 3),
        Some((_, c)) if c != '\'' && s[1 + c.len_utf8()..].starts_with('\'') => c.len_utf8() + 2,
        _ => 0,
    }
}

fn markup(s: &str, out: &mut Spans) {
    let b = s.as_bytes();
    let name_char = |c: u8| ident_char(c) || matches!(c, b':' | b'.' | b'-' | b'@');
    let mut i = 0;
    while i < b.len() {
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(s.len(), |n| i + n + 3);
            out.push((i..end, Token::Comment));
            i = end;
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').map_or(s.len(), |n| i + n + 1);
            out.push((i..end, Token::Meta));
            i = end;
        } else if b[i] == b'<' && rest[1..].trim_start_matches('/').bytes().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1 + usize::from(b[i + 1] == b'/');
            let end = i + s[i..].bytes().take_while(|c| name_char(*c)).count();
            out.push((i..end, Token::Name));
            i = end;
            while i < b.len() && b[i] != b'>' {
                if b[i] == b'"' || b[i] == b'\'' {
                    let end = s[i + 1..].find(b[i] as char).map_or(s.len(), |n| i + n + 2);
                    out.push((i..end, Token::String));
                    i = end;
                } else if name_char(b[i]) {
                    let end = i + s[i..].bytes().take_while(|c| name_char(*c)).count();
                    out.push((i..end, Token::Attr));
                    i = end;
                } else {
                    i += char_len(s, i);
                }
            }
        } else {
            i += char_len(s, i);
        }
    }
}

fn css(l: &Lang, s: &str, out: &mut Spans) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() || b"{};".contains(&c) {
            i += 1;
            continue;
        }
        let rest = &s[i..];
        if let Some(end) = css_comment(l, s, i) {
            out.push((i..end, Token::Comment));
            i = end;
            continue;
        }
        let stop = css_stop(l, s, i);
        if c == b'@' {
            let end = i + 1 + rest[1..].bytes().take_while(|c| ident_char(*c) || *c == b'-').count();
            out.push((i..end, Token::Keyword));
            css_value(l, s, end, stop, out);
        } else if b.get(stop) == Some(&b'{') {
            css_selector(l, s, i, stop, out);
        } else {
            let colon = rest[..stop - i].find(':').map_or(stop, |n| i + n);
            let name = s[i..colon].trim_end();
            let var = name.starts_with('$') || name.starts_with("--");
            out.push((i..i + name.len(), if var { Token::Variable } else { Token::Attribute }));
            css_value(l, s, (colon + 1).min(stop), stop, out);
        }
        i = stop;
    }
}

fn css_comment(l: &Lang, s: &str, i: usize) -> Option<usize> {
    let rest = &s[i..];
    if rest.starts_with("/*") {
        return Some(rest[2..].find("*/").map_or(s.len(), |n| i + n + 4));
    }
    (l.dollar_vars && rest.starts_with("//")).then(|| line_end(s, i))
}

/// Where the statement at `i` ends: the next `{`, `}` or `;` outside strings,
/// comments and parentheses. A `{` makes it a selector.
fn css_stop(l: &Lang, s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    let mut parens = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = string_end(b, i, b[i]);
                continue;
            }
            b'(' => parens += 1,
            b')' => parens -= 1,
            b'{' | b'}' | b';' if parens <= 0 => return i,
            b'#' if b.get(i + 1) == Some(&b'{') => {
                i = s[i..].find('}').map_or(s.len(), |n| i + n + 1);
                continue;
            }
            _ => {
                i = css_comment(l, s, i).unwrap_or(i + char_len(s, i));
                continue;
            }
        }
        i += 1;
    }
    b.len()
}

fn css_selector(l: &Lang, s: &str, mut i: usize, stop: usize, out: &mut Spans) {
    let b = s.as_bytes();
    let word = |from: usize| from + s[from..stop].bytes().take_while(|c| ident_char(*c) || *c == b'-').count();
    while i < stop {
        let c = b[i];
        if let Some(end) = css_comment(l, s, i) {
            out.push((i..end.min(stop), Token::Comment));
            i = end;
        } else if c == b'.' || c == b'#' {
            let end = word(i + 1);
            out.push((i..end, Token::Title));
            i = end;
        } else if c == b':' {
            let from = if b.get(i + 1) == Some(&b':') { i + 2 } else { i + 1 };
            let end = word(from);
            out.push((i..end, Token::Symbol));
            i = end;
        } else if c == b'[' {
            let end = s[i..stop].find(']').map_or(stop, |n| i + n + 1);
            out.push((i..end, Token::Symbol));
            i = end;
        } else if ident_start(c) {
            let end = word(i);
            out.push((i..end, Token::Keyword));
            i = end;
        } else {
            i += char_len(s, i);
        }
    }
}

fn css_value(l: &Lang, s: &str, mut i: usize, stop: usize, out: &mut Spans) {
    let b = s.as_bytes();
    while i < stop {
        let c = b[i];
        let word = |from: usize| from + s[from..stop].bytes().take_while(|c| ident_char(*c) || *c == b'-').count();
        if let Some(end) = css_comment(l, s, i) {
            out.push((i..end, Token::Comment));
            i = end;
        } else if c == b'"' || c == b'\'' {
            let end = string_end(b, i, c);
            out.push((i..end, Token::String));
            i = end;
        } else if c == b'#' && b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) {
            let end = word(i + 1);
            out.push((i..end, Token::Number));
            i = end;
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let end = number_end(b, i + usize::from(c == b'.'));
            out.push((i..end, Token::Number));
            i = end;
        } else if c == b'!' {
            let end = word(i + 1);
            out.push((i..end, Token::Keyword));
            i = end;
        } else if (c == b'$' && l.dollar_vars) || s[i..].starts_with("--") {
            let end = word(i + 1);
            out.push((i..end, Token::Variable));
            i = end;
        } else if ident_start(c) || c == b'-' {
            let end = word(i);
            if b.get(end) == Some(&b'(') {
                out.push((i..end, Token::BuiltIn));
            }
            i = end.max(i + 1);
        } else {
            i += char_len(s, i);
        }
    }
}

/// YAML, TOML and INI: line-oriented keys and sections, values tokenized as code.
fn config(l: &Lang, words: &HashMap<&str, Token>, s: &str, out: &mut Spans) {
    let yaml = l.names[0] == "yaml";
    let mut at = 0;
    for line in s.split('\n') {
        let start = at;
        at += line.len() + 1;
        let mut i = start + line.len() - line.trim_start().len();
        let t = line.trim();
        let end = start + line.trim_end().len();
        if t.is_empty() {
            continue;
        }
        if l.line.iter().any(|m| t.starts_with(m)) {
            out.push((i..end, Token::Comment));
            continue;
        }
        if yaml && (t == "---" || t == "...") {
            out.push((i..end, Token::Meta));
            continue;
        }
        if !yaml && t.starts_with('[') {
            out.push((i..end, Token::Section));
            continue;
        }
        while yaml && (s[i..end].starts_with("- ") || &s[i..end] == "-") {
            out.push((i..i + 1, Token::Bullet));
            i += 1 + s[i + 1..end].len() - s[i + 1..end].trim_start().len();
        }
        let rest = &s[i..end];
        let sep = if yaml { rest.find(": ").or_else(|| rest.strip_suffix(':').map(str::len)) } else { rest.find('=') };
        let mut value = i;
        if let Some(k) = sep.filter(|k| !rest[..*k].contains(" #") && !rest.starts_with(['{', '['])) {
            out.push((i..i + rest[..k].trim_end().len(), Token::Attr));
            value = i + k + 1;
        }
        let v = &s[value..end];
        let lead = v.len() - v.trim_start().len();
        let scalar = v.trim_start();
        // A bare YAML scalar is a string, as highlight.js shows it.
        let plain = yaml
            && !scalar.is_empty()
            && !scalar.starts_with(['"', '\'', '[', '{', '|', '>', '&', '*', '#', '-'])
            && !scalar.as_bytes()[0].is_ascii_digit()
            && !words.contains_key(scalar.split(" #").next().unwrap().trim());
        if plain {
            let text = scalar.split(" #").next().unwrap().trim_end();
            out.push((value + lead..value + lead + text.len(), Token::String));
            if text.len() < scalar.len() {
                code_spans(l, words, &s[value + lead + text.len()..end], value + lead + text.len(), out);
            }
        } else {
            code_spans(l, words, v, value, out);
        }
    }
}

fn markdown(s: &str, out: &mut Spans) {
    static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([-*+]|[0-9]{1,9}[.)])\s").unwrap());
    static INLINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(`[^`]*`)|(\*\*[^*]+\*\*|__[^_]+__)|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)|\[([^\]]*)\]\(([^)]*)\)").unwrap()
    });
    let mut at = 0;
    let mut fence: Option<&str> = None;
    for line in s.split('\n') {
        let start = at;
        at += line.len() + 1;
        let t = line.trim_start();
        let i = start + line.len() - t.len();
        let mark = ["```", "~~~"].into_iter().find(|m| t.starts_with(m));
        if let Some(open) = fence {
            if mark == Some(open) {
                fence = None;
            }
            continue;
        }
        if mark.is_some() {
            fence = mark;
            continue;
        }
        let hashes = t.bytes().take_while(|c| *c == b'#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with([' ', '\t']) {
            out.push((i..start + line.trim_end().len(), Token::Section));
            continue;
        }
        if t.starts_with('>') {
            out.push((i..start + line.trim_end().len(), Token::Quote));
            continue;
        }
        if let Some(m) = BULLET.captures(line).and_then(|c| c.get(1)) {
            out.push((start + m.start()..start + m.end(), Token::Bullet));
        }
        for c in INLINE.captures_iter(line) {
            let span = |n: usize| c.get(n).map(|m| start + m.start()..start + m.end());
            if let Some(r) = span(2) {
                out.push((r, Token::Strong));
            } else if let Some(r) = span(3) {
                out.push((r, Token::Emphasis));
            } else if let (Some(text), Some(url)) = (span(4), span(5)) {
                out.push((text, Token::String));
                out.push((url, Token::Link));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans<'a>(lang: &str, code: &'a str) -> Vec<(&'a str, Token)> {
        highlight(lang, code).into_iter().map(|(r, t)| (&code[r], t)).collect()
    }

    #[test]
    fn code_languages() {
        use Token::*;
        assert_eq!(
            spans("rust", "fn main() { let s: &'a str = \"中\"; // hi\n}"),
            [("fn", Keyword), ("main", Title), ("let", Keyword), ("str", Type), ("\"中\"", String), ("// hi", Comment)]
        );
        assert_eq!(
            spans("py", "@cache\ndef f(x=0x1F):\n    return None  # done"),
            [("@cache", Meta), ("def", Keyword), ("f", Title), ("0x1F", Number), ("return", Keyword), ("None", Literal), ("# done", Comment)]
        );
        assert_eq!(spans("c", "#include <stdio.h>\nint x = 'a';"), [("#include <stdio.h>", Meta), ("int", Type), ("'a'", String)]);
        assert_eq!(spans("SQL", "select * from t -- x"), [("select", Keyword), ("from", Keyword), ("-- x", Comment)]);
        assert_eq!(spans("sh", "echo \"$HOME\" $1 ${x}"), [("echo", BuiltIn), ("\"$HOME\"", String), ("$1", Variable), ("${x}", Variable)]);
        assert_eq!(spans("ps1", "Get-ChildItem $env"), [("Get-ChildItem", BuiltIn), ("$env", Variable)]);
        assert_eq!(spans("json", r#"{"a": [1, true, "b"]}"#), [(r#""a""#, Attr), ("1", Number), ("true", Literal), (r#""b""#, String)]);
        assert_eq!(spans("python", "s = '''a\nb'''"), [("'''a\nb'''", String)]);
    }

    #[test]
    fn markup_and_css() {
        use Token::*;
        assert_eq!(
            spans("html", "<!DOCTYPE html><a href=\"x\">t</a><!-- c -->"),
            [("<!DOCTYPE html>", Meta), ("a", Name), ("href", Attr), ("\"x\"", String), ("a", Name), ("<!-- c -->", Comment)]
        );
        assert_eq!(
            spans("css", "a.btn:hover { color: #fff; margin: 0 4px !important; }\n@media (x) {}"),
            [
                ("a", Keyword),
                (".btn", Title),
                (":hover", Symbol),
                ("color", Attribute),
                ("#fff", Number),
                ("margin", Attribute),
                ("0", Number),
                ("4px", Number),
                ("!important", Keyword),
                ("@media", Keyword),
            ]
        );
        assert_eq!(spans("scss", "$gap: 4px;\n// note"), [("$gap", Variable), ("4px", Number), ("// note", Comment)]);
    }

    #[test]
    fn config_and_markdown() {
        use Token::*;
        assert_eq!(
            spans("yaml", "---\nname: app # c\nlist:\n  - true\n  - 3"),
            [("---", Meta), ("name", Attr), ("app", String), ("# c", Comment), ("list", Attr), ("-", Bullet), ("true", Literal), ("-", Bullet), ("3", Number)]
        );
        assert_eq!(spans("toml", "[deps]\nx = \"1\""), [("[deps]", Section), ("x", Attr), ("\"1\"", String)]);
        assert_eq!(
            spans("md", "# T\n- **b** [l](u)\n```\n# no\n```\n> q"),
            [("# T", Section), ("-", Bullet), ("**b**", Strong), ("l", String), ("u", Link), ("> q", Quote)]
        );
    }

    #[test]
    fn knows_every_detected_language() {
        for lang in ["python", "typescript", "javascript", "jsx", "java", "csharp", "cpp", "c", "php", "ruby", "go", "rust", "kotlin", "swift", "sql", "bash", "powershell", "css", "scss", "html", "xml", "yaml", "toml", "json", "markdown", "lua", "r", "dart", "haskell", "graphql", "elixir", "dockerfile", "ini", "nginx"] {
            assert!(languages().any(|l| l == lang), "{lang}");
        }
        assert!(highlight("brainfuck", "+++").is_empty());
    }

    #[test]
    fn ranges_are_ordered_and_on_char_boundaries() {
        let code = "中文 \"字符串\" // 注释\n/* 块 */ 'x' 1.5e3 中\n<a b='中\n$ ${ @é #中 [x 'é' '\\u{1F600}' 键: 值 # c\n- 中: \"x\na.中:é { 中: 1中; }\n# 中\n**中** [中](é) `中` <!-- 中";
        for lang in languages() {
            let spans = highlight(lang, code);
            for end in ["'", "$", "<", "\"", "@", "#", "/*", "<!--", "0x", "1e", "{", "a:"] {
                highlight(lang, &format!("{code}\n{end}"));
            }
            for pair in spans.windows(2) {
                assert!(pair[0].0.end <= pair[1].0.start, "{lang} {spans:?}");
            }
            for (r, _) in &spans {
                assert!(code.is_char_boundary(r.start) && code.is_char_boundary(r.end), "{lang}");
            }
        }
    }

    /// `cargo test --release -p mdwisp-text speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn speed() {
        let unit = "fn main() {\n    let mut total: u64 = 0; // sum\n    for item in items.iter() { total += item.value(\"x\", 1.5); }\n}\n";
        let code = unit.repeat(1000);
        let t = std::time::Instant::now();
        let first = highlight("rust", unit).len();
        println!("first call: {:?} ({first} spans)", t.elapsed());
        let t = std::time::Instant::now();
        let n = highlight("rust", &code).len();
        println!("{} KB: {:?} ({n} spans)", code.len() / 1024, t.elapsed());
        let t = std::time::Instant::now();
        let r = crate::detect::detect_language(unit);
        println!("detect first call: {:?} ({:?})", t.elapsed(), r.lang);
        let block = unit.replace("main", "run").repeat(10);
        let t = std::time::Instant::now();
        for _ in 0..100 {
            crate::detect::detect_language(&block);
        }
        println!("detect, 40-line block: {:?} each", t.elapsed() / 100);
    }
}
