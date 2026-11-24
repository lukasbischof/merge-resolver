//! merge-resolver: a three-pane terminal merge tool for `git mergetool`.

mod app;
mod doc;
mod input;
mod keys;
mod layout;
mod merge;
mod motion;
mod text;
mod ui;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;

use app::{App, Mode};
use doc::Doc;
use input::FileText;

const USAGE: &str = "\
Usage: merge-resolver [--no-auto-apply] BASE LOCAL REMOTE MERGED

Resolves a three-way merge in the terminal and writes the result to MERGED.

  --no-auto-apply   start with non-conflicting changes unapplied

Git setup (~/.gitconfig):

  [merge]
      tool = merge-resolver
  [mergetool \"merge-resolver\"]
      cmd = merge-resolver \"$BASE\" \"$LOCAL\" \"$REMOTE\" \"$MERGED\"
      trustExitCode = true

Press <space>? inside the tool for the key bindings.";

/// Reads an input file. A missing BASE (both sides added the file) is empty.
fn read(path: &Path, may_be_missing: bool) -> Result<FileText, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if may_be_missing && e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let text =
        String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))?;
    Ok(input::parse(&text))
}

/// The git operation in progress, to label the incoming side.
fn git_operation() -> Option<String> {
    let heads = [
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("REBASE_HEAD", "rebase"),
        ("MERGE_HEAD", "merge"),
    ];
    for (head, name) in heads {
        let out = Command::new("git")
            .args(["rev-parse", "--short", "--verify", "--quiet", head])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if out.status.success() {
            return Some(format!(
                "{name} {}",
                String::from_utf8_lossy(&out.stdout).trim()
            ));
        }
    }
    None
}

fn load(paths: &[PathBuf], auto_apply: bool) -> Result<App, String> {
    let base = read(&paths[0], true)?;
    let left = read(&paths[1], false)?;
    let right = read(&paths[2], false)?;
    let crlf = if left.lines.is_empty() {
        right.crlf
    } else {
        left.crlf
    };
    let final_newline =
        input::merge_final_newline(base.final_newline, left.final_newline, right.final_newline);
    let mut app = App::new(
        Doc::new(base.lines, left.lines, right.lines, auto_apply),
        paths[3].clone(),
    );
    app.eol = if crlf { "\r\n" } else { "\n" };
    app.final_newline = final_newline;
    app.titles[1] = format!("Result  {}", paths[3].display());
    if let Some(op) = git_operation() {
        app.titles[2] = format!("Incoming  {op}");
    }
    Ok(app)
}

fn run(app: &mut App) -> io::Result<i32> {
    let mut terminal = ratatui::try_init()?;
    let mut bar_cursor = false;
    let result = loop {
        if let Err(e) = terminal.draw(|f| ui::draw(f, app)) {
            break Err(e);
        }
        if let Some(code) = app.quit {
            break Ok(code);
        }
        match event::read() {
            Ok(Event::Key(k)) => {
                for key in keys::from_event(k) {
                    app.handle_key(key);
                }
            }
            Ok(_) => {}
            Err(e) => break Err(e),
        }
        let want_bar = matches!(app.mode, Mode::Insert | Mode::Cmd | Mode::Search(_));
        if want_bar != bar_cursor {
            bar_cursor = want_bar;
            let style = if want_bar {
                SetCursorStyle::SteadyBar
            } else {
                SetCursorStyle::SteadyBlock
            };
            let _ = execute!(io::stdout(), style);
        }
    };
    let _ = execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
    ratatui::restore();
    result
}

fn main() -> ExitCode {
    let mut auto_apply = true;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut only_paths = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            _ if only_paths => paths.push(arg.into()),
            "--" => only_paths = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("merge-resolver {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--no-auto-apply" => auto_apply = false,
            _ => paths.push(arg.into()),
        }
    }
    if paths.len() != 4 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let colors = std::env::var("COLORTERM").unwrap_or_default();
    ui::set_truecolor(colors.contains("truecolor") || colors.contains("24bit"));

    let mut app = match load(&paths, auto_apply) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("merge-resolver: {e}");
            return ExitCode::from(2);
        }
    };
    match run(&mut app) {
        Ok(code) => {
            let _ = io::stdout().flush();
            ExitCode::from(code as u8)
        }
        Err(e) => {
            eprintln!("merge-resolver: terminal error: {e}");
            ExitCode::from(2)
        }
    }
}
