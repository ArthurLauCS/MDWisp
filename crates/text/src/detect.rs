//! Local code-language detection. No network, no model.
//!
//! Three tiers, highest confidence first:
//!   1. fingerprint — a token that can only belong to one language
//!   2. weighted    — additive evidence, must beat the runner-up by a margin
//!   3. give up     — return `None` rather than guess; a wrong highlight is
//!                    worse than no highlight, and the UI lets the user pick.

use std::sync::LazyLock;

use regex::bytes::{Regex, RegexBuilder};

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub lang: Option<&'static str>,
    /// 0–100. Only meaningful when `lang` is set.
    pub confidence: u32,
    /// Runner-up and its score, for the "why this language" affordance.
    pub runner_up: Option<(&'static str, u32)>,
}

/// `(lang, strong, weak)`. Patterns follow JavaScript regex semantics: they
/// are compiled without Unicode, so `\w`, `\d`, `\b` are ASCII as in the TS.
const RULES: &[(&str, &[&str], &[&str])] = &[
    ("python", &[r"(?m)^\s*def\s+\w+\s*\(.*\)\s*:", r"(?m)^\s*class\s+\w+(\(.*\))?\s*:", r#"__name__\s*==\s*['"]__main__['"]"#],
        &[r"(?m)^\s*(import|from)\s+\w+", r"\belif\b", r"\brange\(", r"\bself\.", r"(?m)^\s{4}\S"]),
    ("typescript", &[r"\binterface\s+\w+\s*\{", r":\s*(string|number|boolean|void|unknown|any)\b", r"\btype\s+\w+\s*=", r"\benum\s+\w+\s*\{", r"\bas\s+const\b", r"<\w+(\s*,\s*\w+)*>\s*\("],
        &[r"\bconst\s+\w+\s*=", r"=>\s*\{", r"\?\?", r"\?\.", r":\s*\w+\[\]", r"\bimplements\b"]),
    // Declarations alone are shared with TypeScript and every C-like language,
    // so they are corroboration. JS is identified by its module and runtime
    // surface, which TypeScript keeps but which a typed snippet rarely shows.
    ("javascript", &[r"\brequire\s*\(", r"module\.exports", r"\bexports\.\w+\s*=", r"=>\s*\{[^}]*\}", r"console\.log\("],
        &[r"\b(const|let|var)\s+\w+\s*=", r"`[^`]*\$\{", r"\bfunction\s+\w+\s*\("]),
    ("jsx", &[r"return\s*\(\s*<", r"<[A-Z]\w*[\s/>]"], &[r"className=", r"\bprops\b", r"\buseState\("]),
    ("java", &[r"public\s+(static\s+)?(class|void|final)\s", r"import\s+java\.", r"System\.out\."], &[r"@Override", r"private\s+\w+\s+\w+\s*[=;]"]),
    ("csharp", &[r"using\s+System(\.\w+)*\s*;", r"\bnamespace\s+[\w.]+\s*\{", r"Console\.WriteLine"],
        &[r"public\s+void\s", r"\[[A-Z]\w*(\(.*\))?\]", r"\bvar\s+\w+\s*=\s*new\b"]),
    ("cpp", &[r"#include\s*<[\w./]+>", r"std::\w+", r"\btemplate\s*<", r"\bnullptr\b"],
        &[r"(?m)^\s*int\s+main\s*\(", r"\bprintf\(", r"\bmalloc\(", r"->\w+\("]),
    ("c", &[r"#include\s*<[\w./]+\.h>"], &[r"\bprintf\(", r"\bstruct\s+\w+\s*\{", r"\btypedef\b"]),
    ("php", &[r"<\?php", r"\$\w+\s*=\s*[^=]", r"function\s+\w+\s*\(\s*\$", r"->\w+\("], &[r"\becho\s", r"\buse\s+\w+\\", r"\$this->"]),
    ("ruby", &[r"(?m)^\s*def\s+\w+[!?]?", r"\bdo\s*\|", r"(?m)^\s*end\s*$", r"\b@\w+\s*="], &[r"\bputs\s", r#"require\s+['"]"#, r":\w+\s*=>"]),
    ("go", &[r"(?m)^\s*package\s+\w+", r"(?m)^\s*func\s+(\(\w+\s+\*?\w+\)\s+)?\w+\s*\(", r"\berr\s*!=\s*nil\b"],
        &[r":=", r"\bdefer\s", r"\bchan\s", r"\bgo\s+func\b"]),
    ("rust", &[r"\bfn\s+\w+\s*\(", r"\bimpl\s+\w+", r"\blet\s+mut\s+", r"\b(&str|String|Vec<|Option<|Result<)"],
        &[r"println!\(", r"\buse\s+std::", r"\bmatch\s+\w+\s*\{", r"\.unwrap\(\)"]),
    ("kotlin", &[r"(?m)^\s*fun\s+\w+", r"\b(val|var)\s+\w+\s*[:=]", r"\bwhen\s*\("], &[r"\bdata\s+class\b", r"!!", r"\bcompanion\s+object\b"]),
    ("swift", &[r"\bfunc\s+\w+\s*\(", r"\bguard\s+let\b", r"\bif\s+let\b", r"@IBOutlet|@IBAction"], &[r"\blet\s+\w+\s*[:=]", r"\bself\.", r"->\s*Void"]),
    ("sql", &[r"(?im)^\s*SELECT\b[\s\S]*?\bFROM\b", r"(?i)\bINSERT\s+INTO\b", r"(?i)\bCREATE\s+(TABLE|INDEX|VIEW)\b", r"(?im)^\s*UPDATE\b[\s\S]*?\bSET\b"],
        &[r"\bWHERE\b", r"\bJOIN\b", r"(?i)\bGROUP\s+BY\b", r"\bNULL\b"]),
    ("bash", &[r"(?m)^#!\s*/(usr/)?bin/(env\s+)?(bash|sh|zsh)", r"(?m)^\s*(if|while)\s+\[\[?", r"(?m)^\s*\w+\(\)\s*\{"],
        &[r"\becho\s", r"\bexport\s+\w+=", r"\bfi\b", r"\$\(", r"(?m)^\s*\w+=\S+"]),
    ("powershell", &[r"\bGet-\w+", r"\$env:\w+", r"-ErrorAction\b", r"\|?\s*Where-Object\b"], &[r"\bWrite-Host\b", r"\bparam\s*\(", r"\$\w+\s*=\s*@\{"]),
    ("css", &[r"(?m)^[.#]?[\w-]+[^{}]*\{[^}]*:[^}]*;", r"@media[^{]*\{", r"(?m)^:root\s*\{"],
        &[r"\bdisplay:\s*(flex|grid|block|none)", r"\bmargin:", r"\bcolor:\s*#"]),
    ("scss", &[r"\$[\w-]+\s*:\s*[^;]+;", r"@mixin\s+\w+", r"@include\s+\w+"], &[r"&:(hover|focus|active)", r"@extend\s"]),
    ("html", &[r"(?i)<!DOCTYPE\s+html>", r"(?i)<html[\s>]", r"(?m)</\w+>\s*$"], &[r"<div\b", r"<span\b", r#"class=""#, r#"href=""#]),
    ("xml", &[r"<\?xml\s+version=", r"xmlns[:=]"], &[r"</\w+>", r"(?m)^<\w+>"]),
    ("yaml", &[r"(?m)^---\s*$", r"(?m)^\s*[\w-]+:\s*$"], &[r"(?m)^\s*-\s+\w+", r"(?m)^\s*[\w-]+:\s+\S+", r"(?m)^\s*#"]),
    ("toml", &[r"(?m)^\[\[?[\w.-]+\]\]?\s*$"], &[r#"(?m)^\s*[\w-]+\s*=\s*["'\d\[{]"#]),
    ("json", &[r#"^\s*\{\s*""#, r#"^\s*\[\s*\{?\s*""#], &[r#""[^"]+"\s*:\s*"#, r"(?m)^\s*\}", r"(?m)^\s*\]"]),
    ("markdown", &[r"(?m)^#{1,6}\s+\S", r"(?m)^\s*[-*]\s+\S"], &[r"\*\*[^*]+\*\*", r"\[[^\]]+\]\([^)]+\)", r"(?m)^>"]),
    // `function name(` is not characteristic of anything — every C-family
    // language has it. Lua is identified by `local`, `nil`, `then`/`end`.
    ("lua", &[r"(?m)^\s*local\s+\w+\s*=", r"\bnil\b", r"\belseif\b", r"(?m)\bthen\s*$"],
        &[r"\bfunction\s+[\w.:]*\s*\(", r"(?m)^\s*end\s*$", r"\brequire\s*\(", r"\.\."]),
    ("r", &[r"<-", r"\blibrary\(", r"\bdata\.frame\("], &[r"\bc\(", r"\bfunction\s*\(", r"(?m)^\s*#"]),
    ("dart", &[r"\bvoid\s+main\s*\(\s*\)", r"@override", r"\bWidget\s+build\("], &[r"\bfinal\s+\w+\s*=", r"(?m)=>\s*\w+,?$", r"\bimport\s+'package:"]),
    ("haskell", &[r"(?m)^\s*\w+\s*::\s*\w+", r"(?m)\bwhere\s*$", r"<-"], &[r"\bdata\s+\w+\s*=", r"\bimport\s+Data\.", r"\bderiving\b"]),
    ("graphql", &[r"\b(query|mutation|subscription)\s+\w*\s*\{", r"\btype\s+\w+\s*\{[^}]*\}"], &[r"\bfragment\s+\w+\s+on\b", r"(?m)^\s*\w+\s*:\s*\[?\w+!?\]?"]),
    ("elixir", &[r"\bdefmodule\s+\w+", r"\bdefp?\s+\w+.*\bdo\b", r"\|>"], &[r"@moduledoc", r"(?m)^\s*end\s*$", r"\bIO\.(puts|inspect)"]),
    // A bare `FROM` line is not enough — SQL has `FROM` too, and a Markdown
    // document can contain either. Dockerfiles are identified by a run of
    // instructions in upper case at line start.
    ("dockerfile", &[r"(?m)^FROM\s+\S+[\s\S]*^RUN\s+", r"(?m)^FROM\s+\S+[\s\S]*^(COPY|ADD|WORKDIR|ENTRYPOINT|CMD)\s+"],
        &[r"(?m)^ARG\s+", r"(?m)^ENV\s+", r"(?m)^EXPOSE\s+", r"(?m)^VOLUME\s+", r"(?m)^LABEL\s+"]),
    ("ini", &[r"(?m)^\[[\w.\s-]+\]\s*$"], &[r"(?m)^\s*[\w.-]+\s*=\s*\S+", r"(?m)^\s*;"]),
    ("nginx", &[r"(?s)\bserver\s*\{[^}]*listen\s+\d+", r"\blocation\s+[\^~=/]"], &[r"\bproxy_pass\b", r"\bupstream\s+\w+", r"\broot\s+/"]),
];

const FINGERPRINTS: &[(&str, &str)] = &[
    (r"(?m)^\s*#!\s*.*\b(bash|sh|zsh)\b", "bash"),
    (r"(?m)^\s*#!\s*.*\bpython[23]?\b", "python"),
    (r"(?m)^\s*#!\s*.*\bnode\b", "javascript"),
    (r"(?m)^\s*#!\s*.*\bruby\b", "ruby"),
    (r"(?i)<!DOCTYPE\s+html>", "html"),
    (r"<\?php", "php"),
    (r"<\?xml\s+version=", "xml"),
    (r"(?m)^\s*package\s+main\b[\s\S]*\bfunc\s+main\s*\(", "go"),
    (r"\bfn\s+main\s*\(\s*\)", "rust"),
    (r"\bdefmodule\s+\w+\s+do\b", "elixir"),
    (r"(?m)^\s*FROM\s+\S+[\s\S]*^\s*RUN\s+", "dockerfile"),
];

/// The leader must beat the runner-up by this much or we decline to guess.
const MARGIN: u32 = 18;
/// Below this absolute score there is simply not enough evidence.
const MIN_SCORE: u32 = 22;

struct Rule {
    lang: &'static str,
    strong: Vec<Regex>,
    weak: Vec<Regex>,
}

fn re(src: &str) -> Regex {
    RegexBuilder::new(src).unicode(false).build().unwrap()
}

static COMPILED: LazyLock<(Vec<Rule>, Vec<(Regex, &'static str)>)> = LazyLock::new(|| {
    let rules = RULES
        .iter()
        .map(|(lang, strong, weak)| Rule {
            lang,
            strong: strong.iter().map(|s| re(s)).collect(),
            weak: weak.iter().map(|s| re(s)).collect(),
        })
        .collect();
    (rules, FINGERPRINTS.iter().map(|(s, lang)| (re(s), *lang)).collect())
});

/// Cheap structural check: does this parse as JSON?
fn is_json(code: &str) -> bool {
    let trimmed = code.trim();
    (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

pub fn detect_language(code: &str) -> Detection {
    let sample = code.char_indices().nth(8000).map_or(code, |(i, _)| &code[..i]);
    let lines: Vec<&[u8]> = sample
        .split('\n')
        .filter(|l| l.chars().any(|c| !c.is_whitespace()))
        .map(str::as_bytes)
        .collect();
    let line_count = lines.len().max(1);
    let none = Detection { lang: None, confidence: 0, runner_up: None };

    if is_json(sample) {
        return Detection { lang: Some("json"), confidence: 98, runner_up: None };
    }
    let (rules, fingerprints) = &*COMPILED;
    let bytes = sample.as_bytes();
    if let Some((_, lang)) = fingerprints.iter().find(|(re, _)| re.is_match(bytes)) {
        return Detection { lang: Some(lang), confidence: 96, runner_up: None };
    }

    let mut scores: Vec<(&'static str, u32)> = Vec::new();
    for rule in rules {
        let strong = rule.strong.iter().filter(|re| re.is_match(bytes)).count() as u32;
        // A language must show at least one marker that is *characteristic* of it.
        // Weak signals are corroboration, not identification — treating them as
        // candidates lets a broad rule like Lua's `function name(` compete with
        // JavaScript and collapse the margin that keeps the result honest.
        if strong == 0 {
            continue;
        }
        let weak = rule.weak.iter().filter(|re| re.is_match(bytes)).count() as u32;
        // Coverage: the share of lines the rule recognises at all. A rule that
        // fires on one line of forty is a coincidence, not a match.
        let covered = lines
            .iter()
            .filter(|l| rule.strong.iter().chain(&rule.weak).any(|re| re.is_match(l)))
            .count();
        let density = covered as f64 / line_count as f64;
        scores.push((rule.lang, strong * 16 + weak * 5 + (density * 26.0).round() as u32));
    }

    // TypeScript is a syntactic superset of JavaScript, so every TS block also
    // scores for JS. The annotation evidence has to outweigh that inheritance.
    if let Some(js) = scores.iter_mut().find(|(lang, _)| *lang == "javascript") {
        js.1 = (js.1 as f64 * 0.72).round() as u32;
    }

    scores.sort_by(|a, b| b.1.cmp(&a.1));
    let Some(&(top_lang, top)) = scores.first() else { return none };
    let runner_up = scores.get(1).copied();
    let gap = top - runner_up.map_or(0, |r| r.1);
    if gap < MARGIN || top < MIN_SCORE {
        return Detection { runner_up, ..none };
    }

    // Confidence blends absolute evidence with the margin over the runner-up:
    // a high score with a close second is not a confident answer.
    let strength = (top as f64 / 90.0).min(1.0);
    let separation = (gap as f64 / 60.0).min(1.0);
    let confidence = (38.0 + 54.0 * (strength * 0.62 + separation * 0.38)).round() as u32;
    Detection { lang: Some(top_lang), confidence: confidence.clamp(38, 95), runner_up }
}

#[cfg(test)]
mod tests {
    use super::detect_language;

    fn lang(code: &str) -> Option<&'static str> {
        detect_language(code).lang
    }

    #[test]
    fn ignores_blank_lines_around_code() {
        let code = "def greet(name):\n    return name";
        assert_eq!(lang(code), Some("python"));
        assert_eq!(detect_language(&format!("\n{code}\n\n")), detect_language(code));
    }

    #[test]
    fn identifies_unambiguous_code() {
        let ts = "interface DocumentMeta {\n  id: string\n  path: string\n  assetDir: string\n}\n\nexport function metaFor(absPath: string): DocumentMeta {\n  const stem = basename(absPath).replace(/\\.md$/i, '')\n  return { id: docIdFor(absPath), path: absPath }\n}";
        let r = detect_language(ts);
        assert_eq!(r.lang, Some("typescript"));
        assert!(r.confidence > 50);

        let py = "import os\nimport sys\n\ndef load(path):\n    with open(path) as f:\n        return f.read()\n\nclass Loader:\n    def __init__(self, root):\n        self.root = root\n\nif __name__ == '__main__':\n    print(load(sys.argv[1]))";
        assert_eq!(lang(py), Some("python"));
        assert_eq!(lang("#!/usr/bin/env bash\nset -euo pipefail\nfor f in *.md; do\n  echo \"处理 $f\"\ndone"), Some("bash"));
        assert_eq!(lang("SELECT id, name, created_at\nFROM users\nWHERE active = 1\nORDER BY created_at DESC"), Some("sql"));
        assert_eq!(lang(r#"{ "name": "mdview", "version": "0.1.0", "private": true }"#), Some("json"));
        assert_eq!(lang("fn main() {\n    let mut total = 0;\n    for item in items.iter() {\n        total += item.value();\n    }\n    println!(\"{}\", total);\n}"), Some("rust"));
        assert_eq!(lang("package main\n\nimport \"fmt\"\n\nfunc main() {\n    msg := \"hello\"\n    if msg != \"\" {\n        fmt.Println(msg)\n    }\n}"), Some("go"));
        assert_eq!(lang("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n  <head><title>页面</title></head>\n  <body><div class=\"app\">内容</div></body>\n</html>"), Some("html"));
    }

    #[test]
    fn refuses_to_guess_on_thin_evidence() {
        assert_eq!(lang("这段文字只是一段普通的中文说明，没有任何代码结构。"), None);
        assert_eq!(lang("name = value"), None);
        let empty = detect_language("");
        assert_eq!((empty.lang, empty.confidence), (None, 0));
        assert_eq!(lang("npm install"), None);
    }

    #[test]
    fn disambiguates_related_languages() {
        assert_eq!(lang("const users: string[] = []\nfunction add(name: string): void {\n  users.push(name)\n}"), Some("typescript"));
        assert_eq!(lang("const users = []\nfunction add(name) {\n  users.push(name)\n}\nmodule.exports = { add }"), Some("javascript"));
    }

    #[test]
    fn result_shape() {
        assert!(detect_language("SELECT * FROM t WHERE id = 1").confidence <= 100);
        let r = detect_language("#!/bin/sh\necho hi");
        assert!(r.lang.is_some() && r.confidence > 0);
    }
}
