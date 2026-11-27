//! merge-resolver: a three-pane terminal merge tool for `git mergetool`.

mod app;
mod doc;
mod input;
mod keys;
mod layout;
mod merge;
mod motion;
mod syntax;
mod text;
mod ui;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;

use app::{App, Mode};
use doc::Doc;
use input::FileText;

const USAGE: &str = "\
Usage: merge-resolver [--auto-apply] BASE LOCAL REMOTE MERGED

Resolves a three-way merge in the terminal and writes the result to MERGED.

  --auto-apply   apply non-conflicting changes on start

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
        // Coloring the rest of the code goes on until a key is pressed.
        if app.highlighting && !event::poll(Duration::ZERO).unwrap_or(true) {
            continue;
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

#[derive(Debug, PartialEq)]
enum Cli {
    Help,
    Version,
    Usage,
    Run {
        paths: Vec<PathBuf>,
        auto_apply: bool,
    },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Cli {
    let mut auto_apply = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut only_paths = false;
    for arg in args {
        match arg.as_str() {
            _ if only_paths => paths.push(arg.into()),
            "--" => only_paths = true,
            "-h" | "--help" => return Cli::Help,
            "-V" | "--version" => return Cli::Version,
            "--auto-apply" => auto_apply = true,
            _ => paths.push(arg.into()),
        }
    }
    if paths.len() != 4 {
        return Cli::Usage;
    }
    Cli::Run { paths, auto_apply }
}

fn main() -> ExitCode {
    let (paths, auto_apply) = match parse_args(std::env::args().skip(1)) {
        Cli::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Cli::Version => {
            println!("merge-resolver {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Cli::Usage => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Cli::Run { paths, auto_apply } => (paths, auto_apply),
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    fn run(auto_apply: bool) -> Cli {
        Cli::Run {
            paths: ["b", "l", "r", "m"].map(PathBuf::from).to_vec(),
            auto_apply,
        }
    }

    #[test]
    fn nothing_is_applied_on_start_unless_asked_for() {
        assert_eq!(cli(&["b", "l", "r", "m"]), run(false));
    }

    #[test]
    fn the_auto_apply_flag_applies_non_conflicting_changes_on_start() {
        assert_eq!(cli(&["--auto-apply", "b", "l", "r", "m"]), run(true));
    }
}
