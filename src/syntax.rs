//! Syntax highlighting: picks the grammar for a file and colors its lines.

use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use Token::*;

/// Lines longer than this many bytes are left plain: they are rarely
/// written by hand and take long to color.
const MAX_LINE: usize = 10_000;

/// How many lines one update colors at most to get to where it has to.
/// A jump far down a long file shows plain text for a moment instead of
/// holding up the screen.
const CATCH_UP: usize = 1000;

/// The parser is remembered after every so many lines, to start again
/// from there after an edit.
const STRIDE: usize = 16;

/// The grammars that `build.rs` packed.
static SET: LazyLock<SyntaxSet> = LazyLock::new(|| {
    let packed = include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.bin"));
    syntect::dumps::from_uncompressed_data(packed).expect("the packed grammars load")
});

/// What a piece of code is, as far as its color is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token {
    Plain,
    Comment,
    Keyword,
    Str,
    /// Numbers, symbols, escapes and literals like `true` and `nil`.
    Constant,
    /// The name of a function, a tag or a key.
    Entity,
    Type,
    /// A variable that is marked as one, like `@name` or `$name`.
    Variable,
    /// An attribute, a property or an annotation.
    Attribute,
}

/// What the scopes that a grammar gives a piece of code mean. A piece gets
/// the token of its innermost scope that is listed here; of the rules that
/// match a scope, the first one counts.
const RULES: &[(&str, Token)] = &[
    ("comment", Comment),
    // Code embedded in a string or in another language starts afresh.
    ("source", Plain),
    ("text", Plain),
    ("meta.embedded", Plain),
    ("meta.interpolation", Plain),
    ("meta.template.expression", Plain),
    ("punctuation.section.embedded", Keyword),
    ("punctuation.section.interpolation", Keyword),
    ("punctuation.definition.tag", Entity),
    ("string", Str),
    ("constant", Constant),
    ("support.constant", Constant),
    ("keyword.other.unit", Constant),
    ("keyword", Keyword),
    ("storage", Keyword),
    ("variable.language", Keyword),
    ("entity.name.function", Entity),
    ("entity.type.function", Entity),
    ("entity.name.tag", Entity),
    ("support.function", Entity),
    ("support.macro", Entity),
    ("variable.function", Entity),
    ("entity.other.attribute-name", Attribute),
    ("support.type.property-name", Attribute),
    ("meta.annotation", Attribute),
    ("variable.annotation", Attribute),
    ("entity.other.inherited-class", Type),
    ("entity.name", Type),
    ("support.type", Type),
    ("support.class", Type),
    ("variable.other.constant", Type),
    ("variable.other.readwrite.instance", Variable),
    ("variable.other.readwrite.class", Variable),
    ("variable.other.readwrite.global", Variable),
    ("variable.other.readwrite.shell", Variable),
    ("punctuation.definition.variable", Variable),
];

static SCOPES: LazyLock<Vec<(Scope, Token)>> = LazyLock::new(|| {
    let scope = |s| Scope::new(s).expect("a valid scope");
    RULES.iter().map(|&(s, token)| (scope(s), token)).collect()
});

/// Grammars use this for `and` and `sizeof` as well as for `+` and `=`.
static OPERATOR: LazyLock<Scope> =
    LazyLock::new(|| Scope::new("keyword.operator").expect("a valid scope"));

/// A string in here is the name of what follows it, as in JSON.
static KEY: LazyLock<Scope> =
    LazyLock::new(|| Scope::new("meta.mapping.key").expect("a valid scope"));

/// The token for `text`, which is in the given scopes, outermost first.
fn classify(scopes: &[Scope], text: &str) -> Token {
    for (depth, &scope) in scopes.iter().enumerate().rev() {
        if OPERATOR.is_prefix_of(scope) {
            let word = text.starts_with(char::is_alphabetic);
            return if word { Keyword } else { Plain };
        }
        let rule = SCOPES.iter().find(|(s, _)| s.is_prefix_of(scope));
        match rule {
            Some((_, Str)) if scopes[..depth].iter().any(|&s| KEY.is_prefix_of(s)) => {
                return Entity;
            }
            Some((_, token)) => return *token,
            None => {}
        }
    }
    Plain
}

/// Colors one line and moves the parser to the start of the next one.
/// Returns runs of one token each as (end byte, token), or nothing if the
/// grammar fails.
fn color(
    line: &str,
    parse: &mut ParseState,
    scopes: &mut ScopeStack,
) -> Option<Vec<(usize, Token)>> {
    let ops = parse.parse_line(&format!("{line}\n"), &SET).ok()?;
    let mut spans: Vec<(usize, Token)> = Vec::new();
    let mut at = 0;
    let mut cut = |end: usize, scopes: &ScopeStack| {
        let end = end.min(line.len());
        if end <= at {
            return;
        }
        let token = classify(scopes.as_slice(), &line[at..end]);
        match spans.last_mut() {
            Some(last) if last.1 == token => last.0 = end,
            _ => spans.push((end, token)),
        }
        at = end;
    };
    for (i, op) in ops {
        cut(i, scopes);
        scopes.apply(&op).ok()?;
    }
    cut(line.len(), scopes);
    Some(spans)
}

/// The grammar of a file.
#[derive(Clone, Copy)]
pub struct Language(&'static SyntaxReference);

impl Language {
    #[cfg(test)]
    pub fn name(self) -> &'static str {
        &self.0.name
    }
}

const OBJECTIVE_C: &[&str] = &["@interface", "@protocol", "@implementation", "#import"];

/// Picks the language for the file at `path`. Its name decides; `texts`,
/// the versions of the file, settle the cases a name leaves open.
pub fn detect(path: &Path, texts: &[&[String]]) -> Option<Language> {
    let name = path.file_name()?.to_str()?;
    let has = |marks: &[&str]| {
        let mut lines = texts.iter().flat_map(|t| t.iter());
        lines.any(|l| marks.iter().any(|m| l.contains(m)))
    };
    // The whole name ("Gemfile"), then its extensions from the longest
    // ("html.erb") to the shortest ("erb").
    let dots = name.match_indices('.').map(|(i, _)| &name[i + 1..]);
    let by_name = std::iter::once(name)
        .chain(dots)
        .find_map(|end| SET.find_syntax_by_extension(end));
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    let syntax = match extension {
        // Rails runs its YAML files through ERB whatever they are called.
        "yml" | "yaml" if has(&["<%"]) => SET.find_syntax_by_name("YAML (Rails)"),
        "h" if has(OBJECTIVE_C) => SET.find_syntax_by_name("Objective-C"),
        "h" => SET.find_syntax_by_name("C++"),
        _ => by_name,
    };
    let first_line = || texts.iter().find_map(|t| t.first());
    syntax
        .or_else(|| SET.find_syntax_by_first_line(first_line()?))
        .filter(|s| s.name != "Plain Text")
        .map(Language)
}

/// A line that has been colored.
struct Colored {
    text: String,
    spans: Vec<(usize, Token)>,
}

/// Where the grammar is between two lines.
type Parser = (ParseState, ScopeStack);

/// The colors of one pane's lines. They are worked out from the top, as
/// far down as they are needed, and again from where the text changed.
pub struct Highlight {
    language: Option<Language>,
    lines: Vec<Colored>,
    /// The parser after each `STRIDE` lines.
    marks: Vec<Parser>,
    /// The parser after the last colored line; none before the first.
    parser: Option<Parser>,
}

impl Highlight {
    pub fn new(language: Option<Language>) -> Highlight {
        Highlight {
            language,
            lines: Vec::new(),
            marks: Vec::new(),
            parser: None,
        }
    }

    /// Colors the first `upto` of `lines`, or a stretch on the way there,
    /// and then the ones after them until `deadline`. Returns whether all
    /// lines are colored.
    pub fn update(&mut self, lines: &[String], upto: usize, deadline: Instant) -> bool {
        let Some(Language(syntax)) = self.language else {
            return true;
        };
        let same = |(old, new): &(&Colored, &String)| old.text == **new;
        let unchanged = self.lines.iter().zip(lines).take_while(same).count();
        if unchanged < self.lines.len() {
            // Back to where the parser was last remembered before the change.
            self.marks.truncate(unchanged / STRIDE);
            self.lines.truncate(self.marks.len() * STRIDE);
            self.parser = self.marks.last().cloned();
        }
        let upto = upto.min(self.lines.len() + CATCH_UP);
        let (mut parse, mut scopes) = self
            .parser
            .take()
            .unwrap_or_else(|| (ParseState::new(syntax), ScopeStack::new()));
        while self.lines.len() < lines.len()
            && (self.lines.len() < upto || Instant::now() < deadline)
        {
            let line = &lines[self.lines.len()];
            let spans = if line.len() > MAX_LINE {
                Vec::new()
            } else if let Some(spans) = color(line, &mut parse, &mut scopes) {
                spans
            } else {
                // A grammar that fails is of no use for the rest either.
                *self = Highlight::new(None);
                return true;
            };
            let text = line.clone();
            self.lines.push(Colored { text, spans });
            if self.lines.len().is_multiple_of(STRIDE) {
                self.marks.push((parse.clone(), scopes.clone()));
            }
        }
        self.parser = Some((parse, scopes));
        self.lines.len() >= lines.len()
    }

    /// The runs of one token each in a line, as (end byte, token). Empty
    /// for a line that is not colored.
    pub fn spans(&self, line: usize) -> &[(usize, Token)] {
        self.lines.get(line).map_or(&[], |l| &l.spans)
    }
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
