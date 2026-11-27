use std::time::Duration;

use super::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}

fn language(file: &str, text: &str) -> Option<&'static str> {
    detect(Path::new(file), &[&lines(text)]).map(|l| l.name())
}

fn highlight(file: &str, lines: &[String]) -> Highlight {
    let mut hl = Highlight::new(detect(Path::new(file), &[lines]));
    hl.update(lines, lines.len(), Instant::now());
    hl
}

/// The pieces of one line that are not plain, without surrounding blanks.
fn pieces(hl: &Highlight, lines: &[String], n: usize) -> Vec<(String, Token)> {
    let mut at = 0;
    let mut out = Vec::new();
    for &(end, token) in hl.spans(n) {
        if token != Plain {
            out.push((lines[n][at..end].trim().to_string(), token));
        }
        at = end;
    }
    out
}

/// The pieces of `text`, the content of `file`, that are not plain.
fn colored(file: &str, text: &str) -> Vec<(String, Token)> {
    let lines = lines(text);
    let hl = highlight(file, &lines);
    (0..lines.len())
        .flat_map(|n| pieces(&hl, &lines, n))
        .collect()
}

#[track_caller]
fn assert_colored(file: &str, text: &str, expected: &[(&str, Token)]) {
    let got = colored(file, text);
    for (piece, token) in expected {
        assert!(
            got.contains(&(piece.to_string(), *token)),
            "{file}: no {token:?} {piece:?} in {got:#?}"
        );
    }
}

#[test]
fn each_language_is_recognized_by_its_file_name() {
    let names = [
        ("app/models/user.rb", "Ruby"),
        ("Gemfile", "Ruby"),
        ("lib/tasks/import.rake", "Ruby"),
        ("index.js", "JavaScript (Babel)"),
        ("index.mjs", "JavaScript (Babel)"),
        ("App.jsx", "JavaScript (Babel)"),
        ("index.ts", "TypeScript"),
        ("App.tsx", "TypeScriptReact"),
        ("deploy.sh", "Bourne Again Shell (bash)"),
        (".zshrc", "Bourne Again Shell (bash)"),
        ("schema.sql", "SQL"),
        ("View.swift", "Swift"),
        ("main.rs", "Rust"),
        ("main.c", "C"),
        ("main.cpp", "C++"),
        ("main.hpp", "C++"),
        ("site.css", "CSS"),
        ("index.html", "HTML"),
        ("show.html.erb", "HTML (Rails)"),
        ("mail.erb", "HTML (Rails)"),
        ("package.json", "JSON"),
        ("config.yml", "YAML"),
        ("config.yaml", "YAML"),
        ("database.yml.erb", "YAML (Rails)"),
        ("View.m", "Objective-C"),
    ];
    for (file, name) in names {
        assert_eq!(language(file, ""), Some(name), "{file}");
    }
}

#[test]
fn a_file_of_an_unknown_kind_has_no_language() {
    assert_eq!(language("notes.txt", "hello"), None);
    assert_eq!(language("data.unknown", "hello"), None);
    assert_eq!(language("LICENSE", "hello"), None);
}

#[test]
fn a_yaml_file_with_erb_tags_is_erb_yaml() {
    assert_eq!(
        language("database.yml", "pool: <%= ENV['POOL'] %>"),
        Some("YAML (Rails)")
    );
}

#[test]
fn a_header_with_objective_c_in_it_is_objective_c() {
    assert_eq!(
        language("View.h", "@interface View : NSObject"),
        Some("Objective-C")
    );
    assert_eq!(language("view.h", "class View {};"), Some("C++"));
}

#[test]
fn a_script_without_extension_is_recognized_by_its_first_line() {
    assert_eq!(
        language("bin/setup", "#!/usr/bin/env ruby\nputs 1"),
        Some("Ruby")
    );
}

#[test]
fn ruby() {
    assert_colored(
        "a.rb",
        "# note\nclass Foo < Bar\n  def call(arg)\n    @count += 42 if arg == :sym\n    puts 'hi'\n  end\nend",
        &[
            ("# note", Comment),
            ("class", Keyword),
            ("Foo", Type),
            ("def", Keyword),
            ("call", Entity),
            ("@count", Variable),
            ("42", Constant),
            ("if", Keyword),
            (":sym", Constant),
            ("'hi'", Str),
        ],
    );
}

#[test]
fn code_interpolated_into_a_string_is_not_colored_as_the_string() {
    assert_eq!(
        colored("a.rb", r#"x = "a#{y}b""#),
        vec![
            ("\"a".to_string(), Str),
            ("#{".to_string(), Keyword),
            ("}".to_string(), Keyword),
            ("b\"".to_string(), Str),
        ]
    );
}

#[test]
fn javascript() {
    assert_colored(
        "a.js",
        "/* note */\nimport x from 'y';\nconst n = 42;\nfunction go(a) { return null; }",
        &[
            ("/* note */", Comment),
            ("import", Keyword),
            ("'y'", Str),
            ("const", Keyword),
            ("42", Constant),
            ("go", Entity),
            ("return", Keyword),
            ("null", Constant),
        ],
    );
}

#[test]
fn typescript() {
    assert_colored(
        "a.ts",
        "// note\nexport interface Foo { bar: string }\nconst n: number = 42;\nconst s = 'y';",
        &[
            ("// note", Comment),
            ("export", Keyword),
            ("interface", Keyword),
            ("Foo", Type),
            ("string", Type),
            ("42", Constant),
            ("'y'", Str),
        ],
    );
}

#[test]
fn bash() {
    assert_colored(
        "a.sh",
        "# note\nfor f in *.txt; do echo \"$f\"; done\nif true; then exit 1; fi",
        &[
            ("# note", Comment),
            ("for", Keyword),
            ("do", Keyword),
            ("$f", Variable),
            ("if", Keyword),
            ("echo", Entity),
        ],
    );
}

#[test]
fn sql() {
    assert_colored(
        "a.sql",
        "-- note\nSELECT id FROM users WHERE age > 18 AND name = 'x';",
        &[
            ("-- note", Comment),
            ("SELECT", Keyword),
            ("FROM", Keyword),
            ("AND", Keyword),
            ("18", Constant),
            ("'x'", Str),
        ],
    );
}

#[test]
fn swift() {
    assert_colored(
        "a.swift",
        "// note\nimport Foundation\nstruct Foo { let x: Int = 42 }\nfunc go(_ a: String) -> Bool { return \"s\" == a }",
        &[
            ("// note", Comment),
            ("import", Keyword),
            ("struct", Keyword),
            ("let", Keyword),
            ("Int", Type),
            ("String", Type),
            ("42", Constant),
            ("func", Keyword),
            ("go", Entity),
            ("return", Keyword),
            ("\"s\"", Str),
        ],
    );
}

#[test]
fn rust() {
    assert_colored(
        "a.rs",
        "// note\nuse std::fmt;\npub fn go(x: &str) -> Option<u32> { let y = 42; println!(\"s\"); None }\nstruct Foo;",
        &[
            ("// note", Comment),
            ("use", Keyword),
            ("pub", Keyword),
            ("fn", Keyword),
            ("go", Entity),
            ("Option", Type),
            ("u32", Keyword),
            ("let", Keyword),
            ("println!", Entity),
            ("42", Constant),
            ("\"s\"", Str),
            ("struct", Keyword),
            ("Foo", Type),
        ],
    );
}

#[test]
fn c() {
    assert_colored(
        "a.c",
        "/* note */\n#include <stdio.h>\nstatic int go(const char *s) { if (s) return 42; puts(\"s\"); }",
        &[
            ("/* note */", Comment),
            ("#include", Keyword),
            ("<stdio.h>", Str),
            ("static", Keyword),
            ("int", Keyword),
            ("go", Entity),
            ("if", Keyword),
            ("return", Keyword),
            ("42", Constant),
            ("\"s\"", Str),
        ],
    );
}

#[test]
fn cpp() {
    assert_colored(
        "a.cpp",
        "// note\nnamespace n {\nclass Foo : public Bar {\npublic:\n  virtual void go();\n};\n}\nconst auto x = 0x1F; auto s = \"s\";",
        &[
            ("// note", Comment),
            ("namespace", Keyword),
            ("class", Keyword),
            ("Foo", Type),
            ("public", Keyword),
            ("virtual", Keyword),
            ("0x1F", Constant),
            ("\"s\"", Str),
        ],
    );
}

#[test]
fn css() {
    assert_colored(
        "a.css",
        "/* note */\n.box > a:hover { color: #fff; margin: 0 auto; background: url(\"x.png\"); }",
        &[
            ("/* note */", Comment),
            (".box", Attribute),
            ("a", Entity),
            ("color", Attribute),
            ("#fff", Constant),
            ("0", Constant),
            ("auto", Constant),
            ("\"x.png\"", Str),
        ],
    );
}

#[test]
fn html() {
    assert_colored(
        "a.html",
        "<!-- note -->\n<a href=\"x\">T &amp; t</a>\n<script>var x = 1;</script>",
        &[
            ("<!-- note -->", Comment),
            ("<a", Entity),
            ("href", Attribute),
            ("\"x\"", Str),
            ("&amp;", Constant),
            ("var", Keyword),
            ("1", Constant),
        ],
    );
}

#[test]
fn erb() {
    assert_colored(
        "a.html.erb",
        "<div class=\"a\"><%= link_to \"x\", root_path if @user %></div>\n<%# note %>",
        &[
            ("<div", Entity),
            ("class", Attribute),
            ("\"a\"", Str),
            ("<%=", Keyword),
            ("\"x\"", Str),
            ("if", Keyword),
            ("@user", Variable),
            ("%>", Keyword),
            ("<%# note %>", Comment),
        ],
    );
}

#[test]
fn json() {
    assert_colored(
        "a.json",
        r#"{"key": "value", "n": 1.5, "b": true, "z": null}"#,
        &[
            ("\"key\"", Entity),
            ("\"value\"", Str),
            ("1.5", Constant),
            ("true", Constant),
            ("null", Constant),
        ],
    );
}

#[test]
fn yaml() {
    assert_colored(
        "a.yml",
        "# note\nkey: value\nlist:\n  - 42\n  - \"two\"\n  - true",
        &[
            ("# note", Comment),
            ("key", Entity),
            ("value", Str),
            ("list", Entity),
            ("42", Constant),
            ("\"two\"", Str),
            ("true", Constant),
        ],
    );
}

#[test]
fn erb_yaml() {
    assert_colored(
        "database.yml",
        "# note\npool: <%= ENV.fetch(\"MAX\") { 5 } %>\npassword: \"<%= @pw %>\"\n<% if test? %>\ntest: 1\n<% end %>",
        &[
            ("# note", Comment),
            ("pool", Entity),
            ("<%=", Keyword),
            ("ENV", Type),
            ("\"MAX\"", Str),
            ("5", Constant),
            ("%>", Keyword),
            ("password", Entity),
            ("@pw", Variable),
            ("<%", Keyword),
            ("if", Keyword),
            ("test", Entity),
            ("1", Constant),
            ("end", Keyword),
        ],
    );
}

#[test]
fn objective_c() {
    assert_colored(
        "a.m",
        "// note\n#import <Foundation/Foundation.h>\n@interface Foo : NSObject\n- (void)bar:(int)x;\n@end\n@implementation Foo\n- (void)bar:(int)x { NSLog(@\"hi\"); return nil; }\n@end",
        &[
            ("// note", Comment),
            ("#import", Keyword),
            ("@interface", Keyword),
            ("Foo", Type),
            ("void", Keyword),
            ("bar:", Entity),
            ("@\"hi\"", Str),
            ("return", Keyword),
            ("nil", Constant),
            ("@end", Keyword),
        ],
    );
}

#[test]
fn operators_made_of_symbols_stay_plain() {
    assert_eq!(colored("a.rb", "a = b + c"), vec![]);
    assert_colored("a.rb", "a and b", &[("and", Keyword)]);
}

#[test]
fn a_comment_spanning_lines_colors_each_of_them() {
    let lines = lines("int a;\n/* one\n   two */\nint b;");
    let hl = highlight("a.c", &lines);
    assert_eq!(
        pieces(&hl, &lines, 1),
        vec![("/* one".to_string(), Comment)]
    );
    assert_eq!(
        pieces(&hl, &lines, 2),
        vec![("two */".to_string(), Comment)]
    );
    assert_eq!(pieces(&hl, &lines, 3), vec![("int".to_string(), Keyword)]);
}

#[test]
fn spans_cover_the_line_and_end_at_char_boundaries() {
    let lines = lines("x = 'é' # ü");
    let hl = highlight("a.rb", &lines);
    let spans = hl.spans(0);
    assert_eq!(spans.last().map(|s| s.0), Some(lines[0].len()));
    assert!(spans.iter().all(|s| lines[0].is_char_boundary(s.0)));
}

#[test]
fn an_edit_recolors_the_lines_from_there_on() {
    let mut text = lines("int a;\nint b;\nint c;");
    let mut hl = highlight("a.c", &text);
    text[1] = "/* int b;".to_string();
    hl.update(&text, text.len(), Instant::now());
    assert_eq!(pieces(&hl, &text, 0), vec![("int".to_string(), Keyword)]);
    assert_eq!(
        pieces(&hl, &text, 1),
        vec![("/* int b;".to_string(), Comment)]
    );
    assert_eq!(pieces(&hl, &text, 2), vec![("int c;".to_string(), Comment)]);
}

#[test]
fn removed_lines_are_forgotten() {
    let mut text = lines("int a;\nint b;");
    let mut hl = highlight("a.c", &text);
    text.pop();
    assert!(hl.update(&text, text.len(), Instant::now()));
    text.push("// c".to_string());
    hl.update(&text, text.len(), Instant::now());
    assert_eq!(pieces(&hl, &text, 1), vec![("// c".to_string(), Comment)]);
}

#[test]
fn only_the_requested_lines_are_colored_once_time_is_up() {
    let text = lines("int a;\nint b;\nint c;");
    let mut hl = Highlight::new(detect(Path::new("a.c"), &[&text]));
    assert!(!hl.update(&text, 1, Instant::now()));
    assert!(!hl.spans(0).is_empty());
    assert!(hl.spans(1).is_empty());
}

#[test]
fn spare_time_is_spent_on_the_lines_further_down() {
    let text = lines("int a;\nint b;\nint c;");
    let mut hl = Highlight::new(detect(Path::new("a.c"), &[&text]));
    assert!(hl.update(&text, 1, Instant::now() + Duration::from_secs(60)));
    assert!(!hl.spans(2).is_empty());
}

#[test]
fn a_very_long_line_is_left_plain() {
    let text = vec![format!("int a = {};", "1 + ".repeat(5000)), "int b;".into()];
    let hl = highlight("a.c", &text);
    assert_eq!(pieces(&hl, &text, 0), vec![]);
    assert_eq!(pieces(&hl, &text, 1), vec![("int".to_string(), Keyword)]);
}

#[test]
fn a_file_without_language_has_no_spans() {
    let text = lines("hello");
    let mut hl = Highlight::new(None);
    assert!(hl.update(&text, 1, Instant::now()));
    assert!(hl.spans(0).is_empty());
}

#[test]
fn a_jump_far_down_is_caught_up_with_a_stretch_at_a_time() {
    let text: Vec<String> = (0..CATCH_UP * 2 + 1)
        .map(|i| format!("int a{i};"))
        .collect();
    let mut hl = Highlight::new(detect(Path::new("a.c"), &[&text]));
    assert!(!hl.update(&text, text.len(), Instant::now()));
    assert!(!hl.spans(CATCH_UP - 1).is_empty());
    assert!(hl.spans(CATCH_UP).is_empty());
    assert!(!hl.update(&text, text.len(), Instant::now()));
    assert!(hl.update(&text, text.len(), Instant::now()));
    assert!(!hl.spans(CATCH_UP * 2).is_empty());
}

#[test]
fn an_edit_far_down_recolors_from_there_without_losing_what_is_above() {
    let mut text: Vec<String> = (0..100).map(|i| format!("int a{i};")).collect();
    text[30] = "/* open".to_string();
    text[35] = "closed */".to_string();
    let mut hl = highlight("a.c", &text);
    for at in [33, 47, 48, 64, 99] {
        text[at] = format!("return {at};");
        assert!(hl.update(&text, text.len(), Instant::now()));
        let expected: Vec<Vec<(String, Token)>> = {
            let fresh = highlight("a.c", &text);
            (0..text.len()).map(|n| pieces(&fresh, &text, n)).collect()
        };
        for (n, expected) in expected.iter().enumerate() {
            assert_eq!(
                &pieces(&hl, &text, n),
                expected,
                "line {n} after editing {at}"
            );
        }
    }
    text.truncate(40);
    text.insert(31, "more".to_string());
    assert!(hl.update(&text, text.len(), Instant::now()));
    assert_eq!(pieces(&hl, &text, 31), vec![("more".to_string(), Comment)]);
    assert_eq!(pieces(&hl, &text, 40), vec![("int".to_string(), Keyword)]);
}
