use std::env;
use std::fs;
use std::io::{self, Read};

use crate::diagnostic::{render_diagnostic, Diagnostic, DiagnosticKind, Language};
use crate::fixer;
use crate::formatter::format_json;
use crate::parser::parse;
use crate::stats::Stats;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    fix: bool,
    stats: bool,
    lang: Language,
    input: Option<String>,
}

pub fn run() -> i32 {
    match run_with_args(env::args().skip(1).collect()) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn run_with_args(args: Vec<String>) -> Result<(), i32> {
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(diag) => {
            eprint!("{}", render_diagnostic(&diag, "", Language::En));
            return Err(2);
        }
    };

    let original = match read_input(&options) {
        Ok(input) => input,
        Err(diag) => {
            eprint!("{}", render_diagnostic(&diag, "", options.lang));
            return Err(2);
        }
    };

    let original_valid = parse(&original).is_ok();

    let (candidate, fix_count) = if options.fix {
        let result = fixer::fix(&original);
        for edit in &result.edits {
            eprintln!("fix: {} at {}", edit.description, edit.position);
        }
        (result.output, result.edits.len())
    } else {
        (original.clone(), 0)
    };

    let value = match parse(&candidate) {
        Ok(value) => value,
        Err(diagnostics) => {
            for diag in diagnostics {
                eprint!("{}", render_diagnostic(&diag, &candidate, options.lang));
            }
            return Err(1);
        }
    };

    let formatted = format_json(&value);
    println!("{}", formatted);

    if options.stats {
        let stats = Stats::from_value(&value, original.len(), formatted.len());
        eprintln!("{}", stats.render(original_valid, fix_count));
    }

    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Options, Diagnostic> {
    let mut options = Options {
        fix: false,
        stats: false,
        lang: Language::En,
        input: None,
    };

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fix" => options.fix = true,
            "--stats" => options.stats = true,
            "--lang" => {
                let Some(value) = iter.next() else {
                    return Err(Diagnostic::new(
                        "E010",
                        DiagnosticKind::InvalidArgument("--lang requires `zh` or `en`".into()),
                        None,
                    ));
                };
                options.lang = Language::parse(&value).map_err(|msg| {
                    Diagnostic::new("E010", DiagnosticKind::InvalidArgument(msg), None)
                })?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            value if value.starts_with('-') => {
                return Err(Diagnostic::new(
                    "E010",
                    DiagnosticKind::InvalidArgument(format!("unknown option `{}`", value)),
                    None,
                ));
            }
            value => {
                if options.input.is_some() {
                    return Err(Diagnostic::new(
                        "E010",
                        DiagnosticKind::InvalidArgument("only one input path is supported".into()),
                        None,
                    ));
                }
                options.input = Some(value.to_string());
            }
        }
    }

    Ok(options)
}

fn read_input(options: &Options) -> Result<String, Diagnostic> {
    if let Some(path) = &options.input {
        fs::read_to_string(path)
            .map_err(|err| Diagnostic::new("E011", DiagnosticKind::Io(err.to_string()), None))
    } else {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|err| Diagnostic::new("E011", DiagnosticKind::Io(err.to_string()), None))?;
        Ok(input)
    }
}

fn print_help() {
    println!(
        "jqr 0.1.0\n\nUSAGE:\n  jqr [--fix] [--stats] [--lang zh|en] [path]\n\nReads stdin when no path is provided."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_flags() {
        let options = parse_args(vec![
            "--fix".into(),
            "--stats".into(),
            "--lang".into(),
            "zh".into(),
            "data.json".into(),
        ])
        .unwrap();

        assert!(options.fix);
        assert!(options.stats);
        assert_eq!(options.lang, Language::Zh);
        assert_eq!(options.input, Some("data.json".into()));
    }
}
