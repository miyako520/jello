#[cfg(feature = "windows-drop")]
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
#[cfg(feature = "windows-drop")]
use std::io::IsTerminal;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::cli::{write_easy_output, EasyCleanupWarning};
use crate::config::{load_language_config as load_language, save_language_config as save_language};
use crate::diagnostic::{escape_terminal_text, Diagnostic, DiagnosticKind, Language};
use crate::fixer::{self, FixEdit};
use crate::lexer::InputMode;
use crate::parser::MAX_INPUT_BYTES;

#[derive(Debug)]
pub(crate) struct DropFileSuccess {
    pub(crate) output_path: PathBuf,
    pub(crate) edits: Vec<FixEdit>,
    pub(crate) cleanup_warning: Option<EasyCleanupWarning>,
}

#[derive(Debug)]
pub(crate) enum DropFileError {
    Content(Vec<Diagnostic>),
    Io(io::Error),
}

fn diagnostics_are_operational(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == DiagnosticKind::AllocationFailed)
}

pub(crate) fn process_file(path: &Path) -> Result<DropFileSuccess, DropFileError> {
    if !fs::metadata(path).map_err(DropFileError::Io)?.is_file() {
        return Err(DropFileError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input path is not a regular file",
        )));
    }

    let file = File::open(path).map_err(DropFileError::Io)?;
    let source = read_source(file)?;
    let fixed = fixer::fix(&source, InputMode::Json5).map_err(DropFileError::Content)?;
    let mut output = fixed.output;
    output.try_reserve_exact(1).map_err(|_| {
        DropFileError::Content(vec![Diagnostic::new(
            "E020",
            DiagnosticKind::AllocationFailed,
            None,
        )])
    })?;
    output.push('\n');

    let saved = write_easy_output(path, output.as_bytes()).map_err(DropFileError::Io)?;
    Ok(DropFileSuccess {
        output_path: saved.path,
        edits: fixed.edits,
        cleanup_warning: saved.cleanup_warning,
    })
}

fn read_source<R: Read>(mut reader: R) -> Result<String, DropFileError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(DropFileError::Io)?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > MAX_INPUT_BYTES {
            return Err(DropFileError::Content(vec![Diagnostic::new(
                "E014",
                DiagnosticKind::InputTooLarge {
                    max_bytes: MAX_INPUT_BYTES,
                },
                None,
            )]));
        }
        bytes.try_reserve(read).map_err(|_| {
            DropFileError::Io(io::Error::new(
                io::ErrorKind::Other,
                "allocation failed while reading input",
            ))
        })?;
        bytes.extend_from_slice(&buffer[..read]);
    }

    String::from_utf8(bytes).map_err(|_| {
        DropFileError::Content(vec![Diagnostic::new(
            "E015",
            DiagnosticKind::InvalidUtf8,
            None,
        )])
    })
}

#[cfg(test)]
fn run_with_io(
    args: Vec<OsString>,
    config_path: &Path,
    input: &mut dyn Read,
    output: &mut dyn Write,
    is_terminal: bool,
) -> i32 {
    run_with_optional_config(args, Some(config_path), input, output, is_terminal)
}

fn run_with_optional_config(
    args: Vec<OsString>,
    config_path: Option<&Path>,
    input: &mut dyn Read,
    output: &mut dyn Write,
    is_terminal: bool,
) -> i32 {
    run_with_io_result(args, config_path, input, output, is_terminal, save_language).unwrap_or(2)
}

struct DropArguments {
    language_override: Option<Language>,
    paths: Vec<OsString>,
}

fn parse_arguments(args: Vec<OsString>) -> Result<DropArguments, String> {
    let mut language_override = None;
    let mut paths = Vec::new();
    let mut options = true;
    let mut arguments = args.into_iter();
    while let Some(argument) = arguments.next() {
        if options && argument == "--" {
            options = false;
        } else if options && argument == "--lang" {
            let value = arguments
                .next()
                .ok_or_else(|| "`--lang` requires `en` or `zh`".to_string())?;
            let value = value
                .to_str()
                .ok_or_else(|| "`--lang` value must be Unicode".to_string())?;
            language_override = Some(Language::parse(value)?);
        } else if options
            && argument
                .to_str()
                .is_some_and(|value| value.starts_with('-'))
        {
            return Err(format!("unknown option {:?}", argument));
        } else {
            paths.push(argument);
        }
    }
    Ok(DropArguments {
        language_override,
        paths,
    })
}

fn requested_language(args: &[OsString], config_path: Option<&Path>) -> Language {
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if argument == "--lang" {
            if let Some(language) = arguments
                .next()
                .and_then(|value| value.to_str())
                .and_then(|value| Language::parse(value).ok())
            {
                return language;
            }
        }
    }
    config_path
        .and_then(|path| load_language(path).ok())
        .flatten()
        .unwrap_or(Language::En)
}

fn read_line(input: &mut dyn Read) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while input.read(&mut byte)? != 0 {
        if byte[0] == b'\n' {
            break;
        }
        bytes.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn choose_language(input: &mut dyn Read, output: &mut dyn Write) -> io::Result<Language> {
    writeln!(output, "Choose a language / 选择语言")?;
    writeln!(output, "  1) English")?;
    writeln!(output, "  2) 中文")?;
    write!(output, "> ")?;
    output.flush()?;
    Ok(match read_line(input)?.as_str() {
        "2" | "zh" => Language::Zh,
        _ => Language::En,
    })
}

fn pause(input: &mut dyn Read, output: &mut dyn Write, language: Language) -> io::Result<()> {
    writeln!(output)?;
    write!(
        output,
        "{}",
        match language {
            Language::En => "Press Enter to close...",
            Language::Zh => "按 Enter 键关闭……",
        }
    )?;
    output.flush()?;
    let _ = read_line(input)?;
    Ok(())
}

fn run_settings(
    language: &mut Language,
    config_path: Option<&Path>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> io::Result<()> {
    loop {
        writeln!(output)?;
        match language {
            Language::En => {
                writeln!(output, "Settings")?;
                writeln!(output, "  1) Change language")?;
                writeln!(output, "  2) How to use drag and drop")?;
                writeln!(output, "  3) Exit")?;
            }
            Language::Zh => {
                writeln!(output, "设置")?;
                writeln!(output, "  1) 切换语言")?;
                writeln!(output, "  2) 查看拖放说明")?;
                writeln!(output, "  3) 退出")?;
            }
        }
        write!(output, "> ")?;
        output.flush()?;
        match read_line(input)?.as_str() {
            "1" => {
                let selected = choose_language(input, output)?;
                let save_result = config_path
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, "configuration is unavailable")
                    })
                    .and_then(|path| save_language(path, selected));
                match save_result {
                    Ok(()) => {
                        *language = selected;
                        match language {
                            Language::En => writeln!(output, "Language saved.")?,
                            Language::Zh => writeln!(output, "语言已保存。")?,
                        }
                    }
                    Err(error) => {
                        *language = Language::En;
                        writeln!(output, "Could not save language settings: {error}")?;
                    }
                }
            }
            "2" => match language {
                Language::En => writeln!(
                    output,
                    "Drag one or more JSON files onto jello-drop.exe. Each result is saved beside its source as a new .fixed file; originals are never changed."
                )?,
                Language::Zh => writeln!(
                    output,
                    "将一个或多个 JSON 文件拖到 jello-drop.exe 上。结果会以新的 .fixed 文件保存在原文件旁边，原文件不会被修改。"
                )?,
            },
            "3" | "" => return Ok(()),
            _ => match language {
                Language::En => writeln!(output, "Please enter 1, 2, or 3.")?,
                Language::Zh => writeln!(output, "请输入 1、2 或 3。")?,
            },
        }
    }
}

fn run_with_io_result<S>(
    args: Vec<OsString>,
    config_path: Option<&Path>,
    input: &mut dyn Read,
    output: &mut dyn Write,
    is_terminal: bool,
    save_config: S,
) -> io::Result<i32>
where
    S: Fn(&Path, Language) -> io::Result<()>,
{
    let argument_language = requested_language(&args, config_path);
    let arguments = match parse_arguments(args) {
        Ok(arguments) => arguments,
        Err(error) => {
            let error = escape_terminal_text(&error);
            match argument_language {
                Language::En => writeln!(output, "Argument error: {error}")?,
                Language::Zh => writeln!(output, "参数错误：{error}")?,
            }
            return Ok(2);
        }
    };
    let language = if let Some(language) = arguments.language_override {
        language
    } else if let Some(config_path) = config_path {
        match load_language(config_path) {
            Ok(Some(language)) => language,
            Ok(None) if is_terminal => {
                let selected = choose_language(input, output)?;
                match save_config(config_path, selected) {
                    Ok(()) => selected,
                    Err(error) => {
                        writeln!(
                            output,
                            "Warning: could not save language settings ({error}); using English."
                        )?;
                        Language::En
                    }
                }
            }
            Ok(None) => Language::En,
            Err(error) => {
                writeln!(
                    output,
                    "Warning: could not read language settings ({error}); using English."
                )?;
                Language::En
            }
        }
    } else {
        writeln!(
            output,
            "Warning: language settings are unavailable; using English."
        )?;
        Language::En
    };

    let mut language = language;
    writeln!(
        output,
        "{}",
        match language {
            Language::En => "Jello drag-and-drop",
            Language::Zh => "Jello 拖放模式",
        }
    )?;
    writeln!(output)?;

    if arguments.paths.is_empty() {
        if !is_terminal {
            writeln!(output, "No input files were provided.")?;
            return Ok(2);
        }
        run_settings(&mut language, config_path, input, output)?;
        pause(input, output, language)?;
        return Ok(0);
    }

    let total = arguments.paths.len();
    let mut succeeded = 0_usize;
    let mut content_errors = 0_usize;
    let mut io_errors = 0_usize;

    for (index, argument) in arguments.paths.into_iter().enumerate() {
        let path = PathBuf::from(argument);
        writeln!(output, "[{}/{}] {:?}", index + 1, total, path)?;
        match process_file(&path) {
            Ok(success) => {
                succeeded += 1;
                match language {
                    Language::En => writeln!(
                        output,
                        "      Success -> {:?} ({} repair{})",
                        success.output_path,
                        success.edits.len(),
                        if success.edits.len() == 1 { "" } else { "s" }
                    )?,
                    Language::Zh => writeln!(
                        output,
                        "      成功 -> {:?}（{} 项修复）",
                        success.output_path,
                        success.edits.len()
                    )?,
                }
                if let Some(warning) = success.cleanup_warning {
                    match language {
                        Language::En => writeln!(
                            output,
                            "      Warning: temporary file cleanup failed at {:?}: {}",
                            warning.path, warning.error
                        )?,
                        Language::Zh => writeln!(
                            output,
                            "      警告：无法清理临时文件 {:?}：{}",
                            warning.path, warning.error
                        )?,
                    }
                }
            }
            Err(DropFileError::Content(diagnostics)) => {
                let operational = diagnostics_are_operational(&diagnostics);
                if operational {
                    io_errors += 1;
                } else {
                    content_errors += 1;
                }
                let message = diagnostics
                    .iter()
                    .find(|diagnostic| {
                        !operational || diagnostic.kind == DiagnosticKind::AllocationFailed
                    })
                    .map(|diagnostic| diagnostic.message(language))
                    .unwrap_or_else(|| "unknown content error".to_string());
                let message = escape_terminal_text(&message);
                match (language, operational) {
                    (Language::En, true) => writeln!(output, "      Resource error: {message}")?,
                    (Language::Zh, true) => writeln!(output, "      资源错误：{message}")?,
                    (Language::En, false) => {
                        writeln!(output, "      Could not safely repair: {message}")?
                    }
                    (Language::Zh, false) => writeln!(output, "      无法安全修复：{message}")?,
                }
            }
            Err(DropFileError::Io(error)) => {
                io_errors += 1;
                match language {
                    Language::En => writeln!(output, "      I/O error: {error}")?,
                    Language::Zh => writeln!(output, "      I/O 错误：{error}")?,
                }
            }
        }
    }

    writeln!(output)?;
    match language {
        Language::En => {
            writeln!(
                output,
                "Completed: {succeeded} succeeded, {content_errors} content error{}, {io_errors} I/O error{}.",
                if content_errors == 1 { "" } else { "s" },
                if io_errors == 1 { "" } else { "s" }
            )?;
            writeln!(output, "Original files were not changed.")?;
        }
        Language::Zh => {
            writeln!(
                output,
                "完成：{succeeded} 个成功，{content_errors} 个内容错误，{io_errors} 个 I/O 错误。"
            )?;
            writeln!(output, "原文件未被修改。")?;
        }
    }

    if is_terminal {
        pause(input, output, language)?;
    }

    Ok(if io_errors > 0 {
        2
    } else if content_errors > 0 {
        1
    } else {
        0
    })
}

fn config_path_from_local_app_data(local_app_data: Option<OsString>) -> io::Result<PathBuf> {
    local_app_data
        .map(PathBuf::from)
        .map(|path| path.join("Jello").join("config"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "the LOCALAPPDATA environment variable is not set",
            )
        })
}

#[cfg(feature = "windows-drop")]
pub(crate) fn run() -> i32 {
    let config_path = config_path_from_local_app_data(env::var_os("LOCALAPPDATA")).ok();
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let is_terminal = stdin.is_terminal();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    run_with_optional_config(
        arguments,
        config_path.as_deref(),
        &mut input,
        &mut output,
        is_terminal,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn processes_json5_into_a_new_fixed_file_without_touching_source() {
        let directory = temporary_directory("process");
        fs::create_dir(&directory).unwrap();
        let source_path = directory.join("data.json");
        let original = "{name:'Ada',items:[1,2,]}";
        fs::write(&source_path, original).unwrap();

        let success = process_file(&source_path).unwrap();

        assert_eq!(success.output_path, directory.join("data.fixed.json"));
        assert_eq!(fs::read_to_string(&source_path).unwrap(), original);
        assert_eq!(
            fs::read_to_string(&success.output_path).unwrap(),
            "{\n  \"name\": \"Ada\",\n  \"items\": [\n    1,\n    2\n  ]\n}\n"
        );
        assert!(!success.edits.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn batch_continues_after_content_error_and_reports_summary() {
        let directory = temporary_directory("mixed-batch");
        fs::create_dir(&directory).unwrap();
        let good_path = directory.join("good.json");
        let bad_path = directory.join("bad.json");
        let config_path = directory.join("config");
        fs::write(&good_path, "{name:'Ada'}").unwrap();
        fs::write(&bad_path, "{name:}").unwrap();
        save_language(&config_path, Language::En).unwrap();
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        let exit_code = run_with_io(
            vec![good_path.clone().into(), bad_path.clone().into()],
            &config_path,
            &mut input,
            &mut output,
            false,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 1);
        assert!(rendered.contains("[1/2]"));
        assert!(rendered.contains("Success"));
        assert!(rendered.contains("[2/2]"));
        assert!(rendered.contains("Could not safely repair"));
        assert!(rendered.contains("Completed: 1 succeeded, 1 content error, 0 I/O errors."));
        assert!(directory.join("good.fixed.json").exists());
        assert!(!directory.join("bad.fixed.json").exists());
        assert_eq!(fs::read_to_string(good_path).unwrap(), "{name:'Ada'}");
        assert_eq!(fs::read_to_string(bad_path).unwrap(), "{name:}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn content_error_messages_escape_terminal_control_characters() {
        let directory = temporary_directory("terminal-controls");
        fs::create_dir(&directory).unwrap();
        let source_path = directory.join("bad.json");
        let config_path = directory.join("config");
        fs::write(&source_path, b"[\x1b]").unwrap();
        save_language(&config_path, Language::En).unwrap();
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        let exit_code = run_with_io(
            vec![source_path.into()],
            &config_path,
            &mut input,
            &mut output,
            false,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 1);
        assert!(!rendered.contains('\x1b'));
        assert!(rendered.contains(r"\u{001B}"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn argument_errors_use_the_requested_language_and_escape_controls() {
        let directory = temporary_directory("argument-language");
        let config_path = directory.join("config");
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        let exit_code = run_with_io(
            vec![
                OsString::from("--unknown-\u{1b}"),
                OsString::from("--lang"),
                OsString::from("zh"),
            ],
            &config_path,
            &mut input,
            &mut output,
            false,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 2);
        assert!(rendered.contains("参数错误"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains(r"\u{1b}"));
    }

    #[test]
    fn language_override_uses_chinese_without_changing_saved_language() {
        let directory = temporary_directory("language-override");
        fs::create_dir(&directory).unwrap();
        let source_path = directory.join("data.json");
        let config_path = directory.join("config");
        fs::write(&source_path, "{name:'Ada'}").unwrap();
        save_language(&config_path, Language::En).unwrap();
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        let exit_code = run_with_io(
            vec![
                OsString::from("--lang"),
                OsString::from("zh"),
                source_path.into(),
            ],
            &config_path,
            &mut input,
            &mut output,
            false,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("成功"));
        assert!(rendered.contains("完成：1 个成功，0 个内容错误，0 个 I/O 错误。"));
        assert_eq!(load_language(&config_path).unwrap(), Some(Language::En));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn first_terminal_run_prompts_for_language_and_saves_the_choice() {
        let directory = temporary_directory("first-run-language");
        fs::create_dir(&directory).unwrap();
        let source_path = directory.join("data.json");
        let config_path = directory.join("config");
        fs::write(&source_path, "{name:'Ada'}").unwrap();
        let mut input = std::io::Cursor::new(b"2\n\n".to_vec());
        let mut output = Vec::new();

        let exit_code = run_with_io(
            vec![source_path.into()],
            &config_path,
            &mut input,
            &mut output,
            true,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("Choose a language / 选择语言"));
        assert!(rendered.contains("成功"));
        assert!(rendered.contains("按 Enter 键关闭"));
        assert_eq!(load_language(&config_path).unwrap(), Some(Language::Zh));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn no_files_opens_settings_and_can_change_the_saved_language() {
        let directory = temporary_directory("settings-language");
        let config_path = directory.join("config");
        save_language(&config_path, Language::En).unwrap();
        let mut input = std::io::Cursor::new(b"1\n2\n3\n\n".to_vec());
        let mut output = Vec::new();

        let exit_code = run_with_io(Vec::new(), &config_path, &mut input, &mut output, true);

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("Settings"));
        assert!(rendered.contains("语言已保存"));
        assert_eq!(load_language(&config_path).unwrap(), Some(Language::Zh));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_menu_explains_how_to_use_drag_and_drop() {
        let directory = temporary_directory("settings-help");
        let config_path = directory.join("config");
        save_language(&config_path, Language::Zh).unwrap();
        let mut input = std::io::Cursor::new(b"2\n3\n\n".to_vec());
        let mut output = Vec::new();

        let exit_code = run_with_io(Vec::new(), &config_path, &mut input, &mut output, true);

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("将一个或多个 JSON 文件拖到 jello-drop.exe 上"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_first_run_config_save_warns_and_continues_in_english() {
        let directory = temporary_directory("config-save-failure");
        fs::create_dir(&directory).unwrap();
        let config_path = directory.join("config");
        let source_path = directory.join("data.json");
        fs::write(&source_path, "{name:'Ada'}").unwrap();
        let mut input = std::io::Cursor::new(b"2\n\n".to_vec());
        let mut output = Vec::new();

        let exit_code = run_with_io_result(
            vec![source_path.into()],
            Some(&config_path),
            &mut input,
            &mut output,
            true,
            |_path, _language| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "simulated save failure",
                ))
            },
        )
        .unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("could not save language settings"));
        assert!(rendered.contains("Success"));
        assert!(!rendered.contains("      成功"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unavailable_config_location_warns_and_still_processes_in_english() {
        let directory = temporary_directory("missing-config-location");
        fs::create_dir(&directory).unwrap();
        let source_path = directory.join("data.json");
        fs::write(&source_path, "{name:'Ada'}").unwrap();
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        let exit_code = run_with_optional_config(
            vec![source_path.into()],
            None,
            &mut input,
            &mut output,
            false,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("language settings are unavailable"));
        assert!(rendered.contains("Success"));
        assert!(directory.join("data.fixed.json").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_save_failure_always_falls_back_to_english() {
        let mut input = std::io::Cursor::new(b"1\n1\n3\n\n".to_vec());
        let mut output = Vec::new();

        let exit_code = run_with_optional_config(
            vec![OsString::from("--lang"), OsString::from("zh")],
            None,
            &mut input,
            &mut output,
            true,
        );

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(exit_code, 0);
        assert!(rendered.contains("Could not save language settings"));
        assert!(rendered.contains("Settings"));
        assert!(rendered.ends_with("Press Enter to close..."));
    }

    #[test]
    fn language_config_is_missing_until_the_user_selects_one() {
        let directory = temporary_directory("missing-language");
        let config_path = directory.join("config");

        assert_eq!(load_language(&config_path).unwrap(), None);
    }

    #[test]
    fn language_config_round_trips_chinese() {
        let directory = temporary_directory("saved-language");
        let config_path = directory.join("config");

        save_language(&config_path, Language::Zh).unwrap();

        assert_eq!(load_language(&config_path).unwrap(), Some(Language::Zh));
        assert_eq!(fs::read_to_string(&config_path).unwrap(), "language=zh\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_language_config_is_reported() {
        let directory = temporary_directory("corrupt-language");
        fs::create_dir(&directory).unwrap();
        let config_path = directory.join("config");
        fs::write(&config_path, "language=maybe\n").unwrap();

        let error = load_language(&config_path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn config_path_is_under_local_app_data() {
        let local_app_data = PathBuf::from(r"C:\Users\Ada\AppData\Local");

        assert_eq!(
            config_path_from_local_app_data(Some(local_app_data.into())).unwrap(),
            PathBuf::from(r"C:\Users\Ada\AppData\Local")
                .join("Jello")
                .join("config")
        );
        assert!(config_path_from_local_app_data(None).is_err());
    }

    #[test]
    fn allocation_failure_is_an_operational_error() {
        let allocation = Diagnostic::new("E020", DiagnosticKind::AllocationFailed, None);
        let content = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('@'), None);

        assert!(diagnostics_are_operational(&[allocation]));
        assert!(!diagnostics_are_operational(&[content]));
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("jello-drop-{}-{nonce}-{name}", std::process::id()))
    }
}
