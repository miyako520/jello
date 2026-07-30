use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::diagnostic::{
    color_enabled, render_diagnostic, ColorChoice, Diagnostic, DiagnosticKind, Language,
};
use crate::fixer;
use crate::formatter::{format_json_with_options, FormatError, FormatOptions};
use crate::lexer::InputMode;
use crate::parser::{parse_with_mode, MAX_INPUT_BYTES};
use crate::stats::Stats;
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::mem::MaybeUninit;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    easy: bool,
    fix: bool,
    stats: bool,
    check: bool,
    write: bool,
    lang: Language,
    input_mode: InputMode,
    color: ColorChoice,
    format: FormatOptions,
    input: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedArgs {
    Run(Options),
    Help,
    Version,
}

#[derive(Debug)]
enum InputError {
    Content(Diagnostic),
    Io(Diagnostic),
}

struct InputData {
    source: String,
    snapshot: Option<FileSnapshot>,
}

#[derive(Debug)]
struct EasyOutput {
    path: PathBuf,
    cleanup_warning: Option<EasyCleanupWarning>,
}

#[derive(Debug)]
struct EasyCleanupWarning {
    path: PathBuf,
    error: io::Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    links: u64,
    #[cfg(windows)]
    volume_serial: u32,
    #[cfg(windows)]
    file_index: u64,
    #[cfg(windows)]
    links: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WindowsFileTime {
    low_date_time: u32,
    high_date_time: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WindowsFileInformation {
    file_attributes: u32,
    creation_time: WindowsFileTime,
    last_access_time: WindowsFileTime,
    last_write_time: WindowsFileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetFileInformationByHandle(
        file: *mut c_void,
        information: *mut WindowsFileInformation,
    ) -> i32;
}

pub(crate) fn run() -> i32 {
    let args = env::args_os().skip(1).collect();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();

    match run_with_io(args, &mut stdin, &mut stdout, &mut stderr) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn run_with_io(
    args: Vec<OsString>,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), i32> {
    let argument_language = requested_language(&args);
    let argument_color = color_enabled(
        requested_color(&args),
        io::stderr().is_terminal(),
        env::var_os("NO_COLOR").is_some(),
    );
    let options = match parse_args(args) {
        Ok(ParsedArgs::Help) => {
            print_help(stdout).map_err(|_| 2)?;
            return Ok(());
        }
        Ok(ParsedArgs::Version) => {
            print_version(stdout).map_err(|_| 2)?;
            return Ok(());
        }
        Ok(ParsedArgs::Run(options)) => options,
        Err(diag) => {
            let rendered =
                render_diagnostic(&diag, "", "<args>", argument_language, argument_color);
            stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
            return Err(2);
        }
    };

    let source_label = options
        .input
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<stdin>".to_string());
    let use_color = color_enabled(
        options.color,
        io::stderr().is_terminal(),
        env::var_os("NO_COLOR").is_some(),
    );

    let input = match read_input(&options, stdin) {
        Ok(input) => input,
        Err(InputError::Content(diag)) => {
            let rendered = render_diagnostic(&diag, "", &source_label, options.lang, use_color);
            stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
            return Err(1);
        }
        Err(InputError::Io(diag)) => {
            let rendered = render_diagnostic(&diag, "", &source_label, options.lang, use_color);
            stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
            return Err(2);
        }
    };
    let original = input.source;
    let input_snapshot = input.snapshot;

    let (value, fix_count, original_valid) = if options.fix {
        let original_valid =
            options.stats && parse_with_mode(&original, options.input_mode).is_ok();
        match fixer::fix(&original, options.input_mode) {
            Ok(result) => {
                for edit in &result.edits {
                    let description =
                        repair_description(edit.code, &edit.description, options.lang);
                    match options.lang {
                        Language::En => writeln!(
                            stderr,
                            "fix[{}]: {} at {}:{} (byte {})",
                            edit.code, description, edit.line, edit.column, edit.byte
                        ),
                        Language::Zh => writeln!(
                            stderr,
                            "修复[{}]：{}，位置 {}:{}（字节 {}）",
                            edit.code, description, edit.line, edit.column, edit.byte
                        ),
                    }
                    .map_err(|_| 2)?;
                }
                (result.value, result.edits.len(), original_valid)
            }
            Err(diagnostics) => {
                let exit_code = diagnostics_exit_code(&diagnostics);
                render_errors(
                    diagnostics,
                    &original,
                    &source_label,
                    options.lang,
                    use_color,
                    stderr,
                )
                .map_err(|_| 2)?;
                return Err(exit_code);
            }
        }
    } else {
        match parse_with_mode(&original, options.input_mode) {
            Ok(value) => (value, 0, true),
            Err(diagnostics) => {
                let exit_code = diagnostics_exit_code(&diagnostics);
                render_errors(
                    diagnostics,
                    &original,
                    &source_label,
                    options.lang,
                    use_color,
                    stderr,
                )
                .map_err(|_| 2)?;
                return Err(exit_code);
            }
        }
    };

    let formatted = match format_json_with_options(&value, options.format) {
        Ok(formatted) => formatted,
        Err(error) => {
            let (diagnostic, exit_code) = format_error_diagnostic(error);
            let rendered =
                render_diagnostic(&diagnostic, "", &source_label, options.lang, use_color);
            stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
            return Err(exit_code);
        }
    };
    let formatted_size = formatted.len();
    let mut canonical = formatted;
    if canonical.try_reserve_exact(1).is_err() {
        let diagnostic = Diagnostic::new("E020", DiagnosticKind::AllocationFailed, None);
        let rendered = render_diagnostic(&diagnostic, "", &source_label, options.lang, use_color);
        stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
        return Err(2);
    }
    canonical.push('\n');

    let stats_output = options.stats.then(|| {
        Stats::from_value(&value, original.len(), formatted_size).render(original_valid, fix_count)
    });
    if options.write || options.easy {
        if let Some(stats) = &stats_output {
            writeln!(stderr, "{}", stats).map_err(|_| 2)?;
        }
        stderr.flush().map_err(|_| 2)?;
    }

    if options.check {
        if original != canonical {
            let message = match options.lang {
                Language::En => "check: input is not formatted",
                Language::Zh => "检查：输入尚未格式化",
            };
            writeln!(stderr, "{}", message).map_err(|_| 2)?;
            return Err(1);
        }
    } else if options.write {
        let path = options.input.as_deref().expect("validated input path");
        let snapshot = input_snapshot.as_ref().expect("file input snapshot");
        if let Err(error) =
            replace_file_safely(path, canonical.as_bytes(), original.as_bytes(), snapshot)
        {
            let diagnostic = Diagnostic::new("E011", DiagnosticKind::Io(error.to_string()), None);
            let rendered =
                render_diagnostic(&diagnostic, "", &source_label, options.lang, use_color);
            stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
            return Err(2);
        }
    } else if options.easy {
        let path = options.input.as_deref().expect("validated easy input path");
        stdout.write_all(canonical.as_bytes()).map_err(|_| 2)?;
        let outcome = match write_easy_output(path, canonical.as_bytes()) {
            Ok(output_path) => output_path,
            Err(error) => {
                let diagnostic = easy_save_diagnostic(error, options.lang);
                let rendered =
                    render_diagnostic(&diagnostic, "", &source_label, options.lang, use_color);
                stderr.write_all(rendered.as_bytes()).map_err(|_| 2)?;
                return Err(2);
            }
        };
        if let Some(warning) = outcome.cleanup_warning {
            let _ = match options.lang {
                Language::En => writeln!(
                    stderr,
                    "warning: formatted output was saved, but temporary file {:?} could not be removed: {}",
                    warning.path, warning.error
                ),
                Language::Zh => writeln!(
                    stderr,
                    "警告：格式化文件已保存，但无法删除临时文件 {:?}：{}",
                    warning.path, warning.error
                ),
            };
        }
        let _ = match options.lang {
            Language::En => writeln!(stderr, "saved formatted output to {:?}", outcome.path),
            Language::Zh => writeln!(stderr, "已将格式化结果保存到 {:?}", outcome.path),
        };
    } else {
        stdout.write_all(canonical.as_bytes()).map_err(|_| 2)?;
    }

    if !options.write && !options.easy {
        if let Some(stats) = stats_output {
            writeln!(stderr, "{}", stats).map_err(|_| 2)?;
        }
    }

    Ok(())
}

fn easy_save_diagnostic(error: io::Error, language: Language) -> Diagnostic {
    let message = match language {
        Language::En => {
            format!("formatted output was printed, but the new file could not be saved: {error}")
        }
        Language::Zh => format!("格式化结果已显示，但无法保存新文件：{error}"),
    };
    Diagnostic::new("E011", DiagnosticKind::Io(message), None)
}

fn format_error_diagnostic(error: FormatError) -> (Diagnostic, i32) {
    match error {
        FormatError::OutputTooLarge { max_bytes } => (
            Diagnostic::new("E019", DiagnosticKind::OutputTooLarge { max_bytes }, None),
            1,
        ),
        FormatError::AllocationFailed => (
            Diagnostic::new("E020", DiagnosticKind::AllocationFailed, None),
            2,
        ),
    }
}

fn repair_description<'a>(code: &str, fallback: &'a str, language: Language) -> &'a str {
    if language == Language::En {
        return fallback;
    }
    match code {
        "F001" => "已将单引号字符串转换为双引号字符串",
        "F002" => "已为未加引号的对象键添加引号",
        "F003" => "已移除尾随逗号",
        "F004" => "已插入缺失的逗号",
        "F005" => "已规范化受支持的 JSON5 语法",
        _ => fallback,
    }
}

fn diagnostics_exit_code(diagnostics: &[Diagnostic]) -> i32 {
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == DiagnosticKind::AllocationFailed)
    {
        2
    } else {
        1
    }
}

fn render_errors(
    diagnostics: Vec<Diagnostic>,
    source: &str,
    source_label: &str,
    language: Language,
    color: bool,
    writer: &mut dyn Write,
) -> io::Result<()> {
    for diagnostic in diagnostics {
        let rendered = render_diagnostic(&diagnostic, source, source_label, language, color);
        writer.write_all(rendered.as_bytes())?;
    }
    Ok(())
}

fn parse_args(args: Vec<OsString>) -> Result<ParsedArgs, Diagnostic> {
    if let Some(action) = requested_action(&args) {
        return Ok(action);
    }
    let language = requested_language(&args);
    parse_args_with_language(args, language)
}

fn parse_args_with_language(
    args: Vec<OsString>,
    language: Language,
) -> Result<ParsedArgs, Diagnostic> {
    let mut options = Options {
        easy: false,
        fix: false,
        stats: false,
        check: false,
        write: false,
        lang: language,
        input_mode: InputMode::Json,
        color: ColorChoice::Auto,
        format: FormatOptions::default(),
        input: None,
    };
    let mut indent = None;
    let mut compact = false;
    let mut positional_only = false;

    let mut iter = args.into_iter();
    if matches!(iter.as_slice().first(), Some(arg) if arg == OsStr::new("easy")) {
        iter.next();
        options.easy = true;
        options.fix = true;
        options.input_mode = InputMode::Json5;
    }
    while let Some(arg) = iter.next() {
        if positional_only {
            set_input(&mut options, PathBuf::from(arg))?;
            continue;
        }
        let Some(value) = arg.to_str() else {
            set_input(&mut options, PathBuf::from(arg))?;
            continue;
        };
        match value {
            "--" => positional_only = true,

            "--fix" => options.fix = true,
            "--stats" => options.stats = true,
            "--check" => options.check = true,
            "--write" | "-i" => options.write = true,
            "--json5" => options.input_mode = InputMode::Json5,
            "--compact" => compact = true,
            "--indent" => {
                let value = next_value(
                    &mut iter,
                    "--indent requires a value from 0 to 16",
                    "`--indent` 需要 0 到 16 的值",
                )?;
                let value = unicode_value(value, "--indent")?;
                let width = value.parse::<usize>().map_err(|_| {
                    invalid_argument(
                        "--indent requires an integer from 0 to 16",
                        "`--indent` 需要 0 到 16 的整数",
                    )
                })?;
                FormatOptions::pretty(width).map_err(|_| {
                    invalid_argument(
                        "--indent requires an integer from 0 to 16",
                        "`--indent` 需要 0 到 16 的整数",
                    )
                })?;
                indent = Some(width);
            }
            "--color" => {
                let value = next_value(
                    &mut iter,
                    "--color requires `auto`, `always`, or `never`",
                    "`--color` 需要 `auto`、`always` 或 `never`",
                )?;
                let value = unicode_value(value, "--color")?;
                options.color = ColorChoice::parse(&value).map_err(|_| {
                    invalid_argument(
                        format!(
                            "unsupported color mode `{}`; expected `auto`, `always`, or `never`",
                            value
                        ),
                        format!(
                            "不支持颜色模式 `{}`；应为 `auto`、`always` 或 `never`",
                            value
                        ),
                    )
                })?;
            }
            "--lang" => {
                let value = next_value(
                    &mut iter,
                    "--lang requires `zh` or `en`",
                    "`--lang` 需要 `zh` 或 `en`",
                )?;
                let value = unicode_value(value, "--lang")?;
                options.lang = Language::parse(&value).map_err(|_| {
                    invalid_argument(
                        format!("unsupported language `{}`; expected `zh` or `en`", value),
                        format!("不支持语言 `{}`；应为 `zh` 或 `en`", value),
                    )
                })?;
            }
            "--help" | "-h" => return Ok(ParsedArgs::Help),
            "--version" | "-V" => return Ok(ParsedArgs::Version),
            value if value.starts_with('-') => {
                return Err(invalid_argument(
                    format!("unknown option `{}`", value),
                    format!("未知选项 `{}`", value),
                ));
            }
            value => set_input(&mut options, PathBuf::from(value))?,
        }
    }

    if options.check && options.write {
        return Err(invalid_argument(
            "--check and --write cannot be used together",
            "`--check` 和 `--write` 不能同时使用",
        ));
    }
    if options.write && options.input.is_none() {
        return Err(invalid_argument(
            "--write requires an input path",
            "`--write` 需要输入路径",
        ));
    }
    if options.easy && options.input.is_none() {
        return Err(invalid_argument(
            "`easy` requires an input path",
            "`easy` 需要输入文件路径",
        ));
    }
    if options.easy && (options.check || options.write) {
        return Err(invalid_argument(
            "`easy` cannot be combined with `--check` or `--write`",
            "`easy` 不能与 `--check` 或 `--write` 同时使用",
        ));
    }
    if compact && indent.is_some() {
        return Err(invalid_argument(
            "--compact and --indent cannot be used together",
            "`--compact` 和 `--indent` 不能同时使用",
        ));
    }
    options.format = if compact {
        FormatOptions::compact()
    } else {
        FormatOptions::pretty(indent.unwrap_or(2)).expect("validated indent width")
    };

    Ok(ParsedArgs::Run(options))
}

fn next_value(
    iter: &mut impl Iterator<Item = OsString>,
    en: &str,
    zh: &str,
) -> Result<OsString, Diagnostic> {
    iter.next().ok_or_else(|| invalid_argument(en, zh))
}

fn unicode_value(value: OsString, option: &str) -> Result<String, Diagnostic> {
    value.into_string().map_err(|_| {
        invalid_argument(
            format!("the value for `{}` must be Unicode text", option),
            format!("`{}` 的值必须是 Unicode 文本", option),
        )
    })
}

fn set_input(options: &mut Options, value: PathBuf) -> Result<(), Diagnostic> {
    if options.input.is_some() {
        return Err(invalid_argument(
            "only one input path is supported",
            "仅支持一个输入路径",
        ));
    }
    options.input = Some(value);
    Ok(())
}

fn invalid_argument(en: impl Into<String>, zh: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        "E010",
        DiagnosticKind::InvalidArgument {
            en: en.into().into_boxed_str(),
            zh: zh.into().into_boxed_str(),
        },
        None,
    )
}

fn requested_action(args: &[OsString]) -> Option<ParsedArgs> {
    let mut help = false;
    let mut version = false;
    for arg in args {
        if arg == OsStr::new("--") {
            break;
        }
        if arg == OsStr::new("--help") || arg == OsStr::new("-h") {
            help = true;
        } else if arg == OsStr::new("--version") || arg == OsStr::new("-V") {
            version = true;
        }
    }
    if help {
        Some(ParsedArgs::Help)
    } else if version {
        Some(ParsedArgs::Version)
    } else {
        None
    }
}

fn requested_color(args: &[OsString]) -> ColorChoice {
    let mut color = ColorChoice::Auto;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == OsStr::new("--") {
            break;
        }
        if arg == OsStr::new("--color") {
            if let Some(value) = iter.next().and_then(|value| value.to_str()) {
                if let Ok(parsed) = ColorChoice::parse(value) {
                    color = parsed;
                }
            }
        }
    }
    color
}

fn requested_language(args: &[OsString]) -> Language {
    let mut language = Language::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == OsStr::new("--") {
            break;
        }
        if arg == OsStr::new("--lang") {
            if let Some(value) = iter.next().and_then(|value| value.to_str()) {
                if let Ok(parsed) = Language::parse(value) {
                    language = parsed;
                }
            }
        }
    }
    language
}

fn read_input(options: &Options, stdin: &mut dyn Read) -> Result<InputData, InputError> {
    if let Some(path) = &options.input {
        if options.write {
            read_file_input(path)
        } else {
            let file = File::open(path).map_err(input_io_error)?;
            read_limited(file).map(|source| InputData {
                source,
                snapshot: None,
            })
        }
    } else {
        read_limited(stdin).map(|source| InputData {
            source,
            snapshot: None,
        })
    }
}

fn read_file_input(path: &Path) -> Result<InputData, InputError> {
    ensure_regular_path(path).map_err(input_io_error)?;
    let mut file = File::open(path).map_err(input_io_error)?;
    let before = FileSnapshot::capture(&file, &file.metadata().map_err(input_io_error)?)
        .map_err(input_io_error)?;
    ensure_regular_path(path).map_err(input_io_error)?;
    let source = read_limited(&mut file)?;
    let after = FileSnapshot::capture(&file, &file.metadata().map_err(input_io_error)?)
        .map_err(input_io_error)?;
    if before != after {
        return Err(input_io_error(concurrent_modification_error()));
    }
    Ok(InputData {
        source,
        snapshot: Some(after),
    })
}

fn input_io_error(error: io::Error) -> InputError {
    InputError::Io(Diagnostic::new(
        "E011",
        DiagnosticKind::Io(error.to_string()),
        None,
    ))
}

fn read_limited<R: Read>(mut reader: R) -> Result<String, InputError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(input_io_error)?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > MAX_INPUT_BYTES {
            return Err(InputError::Content(Diagnostic::new(
                "E014",
                DiagnosticKind::InputTooLarge {
                    max_bytes: MAX_INPUT_BYTES,
                },
                None,
            )));
        }
        bytes.try_reserve(read).map_err(|_| {
            InputError::Io(Diagnostic::new(
                "E020",
                DiagnosticKind::AllocationFailed,
                None,
            ))
        })?;
        bytes.extend_from_slice(&buffer[..read]);
    }

    if bytes.len() > MAX_INPUT_BYTES {
        return Err(InputError::Content(Diagnostic::new(
            "E014",
            DiagnosticKind::InputTooLarge {
                max_bytes: MAX_INPUT_BYTES,
            },
            None,
        )));
    }

    String::from_utf8(bytes).map_err(|_| {
        InputError::Content(Diagnostic::new("E015", DiagnosticKind::InvalidUtf8, None))
    })
}

fn write_easy_output(path: &Path, contents: &[u8]) -> io::Result<EasyOutput> {
    write_easy_output_with(
        path,
        contents,
        |file, contents| file.write_all(contents),
        |path| fs::remove_file(path),
    )
}

fn write_easy_output_with<F, R>(
    path: &Path,
    contents: &[u8],
    write_contents: F,
    remove_published_temporary: R,
) -> io::Result<EasyOutput>
where
    F: FnOnce(&mut File, &[u8]) -> io::Result<()>,
    R: FnOnce(&Path) -> io::Result<()>,
{
    let permissions = fs::metadata(path)?.permissions();
    let (temporary_path, mut temporary) = create_temporary_sibling(path, "easy")?;
    let prepare_result = (|| {
        temporary.set_permissions(permissions)?;
        write_contents(&mut temporary, contents)?;
        temporary.flush()?;
        temporary.sync_all()
    })();
    drop(temporary);

    if let Err(error) = prepare_result {
        return Err(remove_easy_temporary(&temporary_path, error));
    }

    for attempt in 0..1000 {
        let candidate = easy_output_path(path, attempt);
        match fs::hard_link(&temporary_path, &candidate) {
            Ok(()) => {
                let cleanup_warning =
                    remove_published_temporary(&temporary_path)
                        .err()
                        .map(|error| EasyCleanupWarning {
                            path: temporary_path,
                            error,
                        });
                return Ok(EasyOutput {
                    path: candidate,
                    cleanup_warning,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(remove_easy_temporary(&temporary_path, error)),
        }
    }

    Err(remove_easy_temporary(
        &temporary_path,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve an easy-mode output file name",
        ),
    ))
}

fn remove_easy_temporary(path: &Path, error: io::Error) -> io::Error {
    match fs::remove_file(path) {
        Ok(()) => error,
        Err(cleanup_error) => io::Error::new(
            error.kind(),
            format!(
                "{error}; failed to remove temporary file {:?}: {cleanup_error}",
                path
            ),
        ),
    }
}

fn easy_output_path(path: &Path, attempt: usize) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(
        path.file_stem()
            .or_else(|| path.file_name())
            .unwrap_or_else(|| OsStr::new("output")),
    );
    name.push(".fixed");
    if attempt > 0 {
        name.push(format!("-{}", attempt + 1));
    }
    if let Some(extension) = path.extension().filter(|extension| !extension.is_empty()) {
        name.push(".");
        name.push(extension);
    }
    parent.join(name)
}

fn replace_file_safely(
    path: &Path,
    contents: &[u8],
    expected_contents: &[u8],
    expected_snapshot: &FileSnapshot,
) -> io::Result<()> {
    verify_file_unchanged(path, expected_contents, expected_snapshot)?;
    let metadata = fs::metadata(path)?;
    let (temporary_path, mut temporary) = create_temporary_sibling(path, "tmp")?;
    let write_result = (|| {
        temporary.set_permissions(metadata.permissions())?;
        temporary.write_all(contents)?;
        temporary.flush()?;
        temporary.sync_all()
    })();
    drop(temporary);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    commit_prepared_replacement(path, &temporary_path, expected_contents, expected_snapshot)
}

fn commit_prepared_replacement(
    path: &Path,
    temporary_path: &Path,
    expected_contents: &[u8],
    expected_snapshot: &FileSnapshot,
) -> io::Result<()> {
    verify_file_unchanged(path, expected_contents, expected_snapshot)?;
    if fs::rename(temporary_path, path).is_ok() {
        return Ok(());
    }

    let backup_path = reserve_sibling_path(path, "bak")?;
    verify_file_unchanged(path, expected_contents, expected_snapshot)?;
    if let Err(error) = fs::rename(path, &backup_path) {
        let _ = fs::remove_file(temporary_path);
        return Err(error);
    }
    match fs::rename(temporary_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(backup_path);
            Ok(())
        }
        Err(error) => Err(rollback_failed_replacement(
            path,
            &backup_path,
            temporary_path,
            error,
        )),
    }
}

fn rollback_failed_replacement(
    path: &Path,
    backup_path: &Path,
    temporary_path: &Path,
    install_error: io::Error,
) -> io::Error {
    match fs::rename(backup_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(temporary_path);
            install_error
        }
        Err(rollback_error) => io::Error::new(
            io::ErrorKind::Other,
            format!(
                "failed to install replacement: {install_error}; rollback failed: {rollback_error}; original remains at `{}` and replacement remains at `{}`",
                backup_path.display(),
                temporary_path.display()
            ),
        ),
    }
}

fn ensure_regular_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(other_io_error("refusing to replace a symbolic link"));
    }
    if !metadata.file_type().is_file() {
        return Err(other_io_error("--write requires a regular file"));
    }
    Ok(())
}

impl FileSnapshot {
    fn capture(_file: &File, metadata: &fs::Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        #[cfg(unix)]
        if metadata.nlink() != 1 {
            return Err(other_io_error(
                "refusing to replace a file with multiple hard links",
            ));
        }

        #[cfg(windows)]
        let windows = windows_file_information(_file)?;
        #[cfg(windows)]
        if windows.number_of_links != 1 {
            return Err(other_io_error(
                "refusing to replace a file with multiple hard links",
            ));
        }

        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            links: metadata.nlink(),
            #[cfg(windows)]
            volume_serial: windows.volume_serial_number,
            #[cfg(windows)]
            file_index: ((windows.file_index_high as u64) << 32) | windows.file_index_low as u64,
            #[cfg(windows)]
            links: windows.number_of_links,
        })
    }
}

#[cfg(windows)]
fn windows_file_information(file: &File) -> io::Result<WindowsFileInformation> {
    let mut information = MaybeUninit::<WindowsFileInformation>::uninit();
    let succeeded =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { information.assume_init() })
    }
}

fn verify_file_unchanged(
    path: &Path,
    expected_contents: &[u8],
    expected_snapshot: &FileSnapshot,
) -> io::Result<()> {
    ensure_regular_path(path)?;
    let mut file = File::open(path)?;
    let before = FileSnapshot::capture(&file, &file.metadata()?)?;
    if &before != expected_snapshot {
        return Err(concurrent_modification_error());
    }

    let mut offset = 0;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(expected) = expected_contents.get(offset..offset + read) else {
            return Err(concurrent_modification_error());
        };
        if expected != &buffer[..read] {
            return Err(concurrent_modification_error());
        }
        offset += read;
    }
    if offset != expected_contents.len() {
        return Err(concurrent_modification_error());
    }

    let after = FileSnapshot::capture(&file, &file.metadata()?)?;
    ensure_regular_path(path)?;
    if after != before {
        return Err(concurrent_modification_error());
    }
    Ok(())
}

fn concurrent_modification_error() -> io::Error {
    other_io_error("input file changed after it was read; refusing to overwrite it")
}

fn other_io_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, message)
}

fn create_temporary_sibling(path: &Path, suffix: &str) -> io::Result<(PathBuf, File)> {
    for attempt in 0..1000 {
        let candidate = sibling_path(path, suffix, attempt);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary file name",
    ))
}

fn reserve_sibling_path(path: &Path, suffix: &str) -> io::Result<PathBuf> {
    let (candidate, file) = create_temporary_sibling(path, suffix)?;
    drop(file);
    fs::remove_file(&candidate)?;
    Ok(candidate)
}

fn sibling_path(path: &Path, suffix: &str, attempt: usize) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_else(|| OsStr::new("input")));
    name.push(format!(
        ".jello.{}.{}.{}",
        std::process::id(),
        attempt,
        suffix
    ));
    parent.join(name)
}

fn print_version(writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer, "jello {}", env!("CARGO_PKG_VERSION"))
}

fn print_help(writer: &mut dyn Write) -> io::Result<()> {
    writeln!(
        writer,
        "jello {version}\n\n\
USAGE:\n  jello [OPTIONS] [--] [path]\n  jello easy [OPTIONS] [--] <path>\n\n\
COMMANDS:\n  easy                   Repair, print, and save as a new .fixed file\n\n\
OPTIONS:\n  --fix                  Repair supported mistakes before formatting\n  --stats                Print structural statistics to stderr\n  --check                Exit 1 when input is not already formatted\n  --write, -i            Replace a checked regular input file\n  --json5                Accept the documented JSON5 subset\n  --indent <0..16>       Pretty-print indentation width (default: 2)\n  --compact              Emit compact JSON\n  --lang <zh|en>         Diagnostic language\n  --color <MODE>         auto, always, or never\n  --version, -V          Print version\n  --help, -h             Print help\n\n\
Reads stdin when no path is provided.\n\
Exit codes: 0 success, 1 invalid content/check failed, 2 argument or I/O error.",
        version = env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stdout_failure_returns_io_exit_code() {
        let mut stdin = Cursor::new(b"{}".as_slice());
        let mut stdout = BrokenWriter;
        let mut stderr = Vec::new();

        let result = run_with_io(Vec::new(), &mut stdin, &mut stdout, &mut stderr);

        assert_eq!(result, Err(2));
    }

    #[test]
    fn easy_stdout_failure_does_not_create_an_output_file() {
        let path = temporary_test_path("easy-stdout.json");
        fs::write(&path, "{}").unwrap();
        let output_path = easy_output_path(&path, 0);
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = BrokenWriter;
        let mut stderr = Vec::new();

        let result = run_with_io(
            vec!["easy".into(), path.clone().into()],
            &mut stdin,
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(result, Err(2));
        assert!(!output_path.exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn easy_output_name_omits_an_empty_extension() {
        assert_eq!(
            easy_output_path(Path::new("data."), 0),
            PathBuf::from("data.fixed")
        );
    }

    #[test]
    fn easy_output_inherits_source_readonly_permission() {
        let path = temporary_test_path("easy-permissions.json");
        fs::write(&path, "{}").unwrap();
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&path, readonly_permissions).unwrap();

        let output_path = write_easy_output(&path, b"{}\n").unwrap().path;
        let inherited_readonly = fs::metadata(&output_path).unwrap().permissions().readonly();

        fs::set_permissions(&path, original_permissions.clone()).unwrap();
        fs::set_permissions(&output_path, original_permissions).unwrap();
        assert!(!sibling_path(&path, "easy", 0).exists());
        fs::remove_file(path).unwrap();
        fs::remove_file(output_path).unwrap();
        assert!(inherited_readonly);
    }

    #[test]
    fn easy_write_failure_leaves_no_fixed_or_temporary_file() {
        let directory = temporary_test_path("easy-partial-write");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("data.json");
        fs::write(&path, "{}").unwrap();

        let result = write_easy_output_with(
            &path,
            b"complete",
            |file, _contents| {
                file.write_all(b"partial")?;
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "injected write failure",
                ))
            },
            |path| fs::remove_file(path),
        );

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WriteZero);
        let entries: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![OsString::from("data.json")]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn easy_cleanup_failure_is_reported_after_successful_publish() {
        let directory = temporary_test_path("easy-cleanup-warning");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("data.json");
        fs::write(&path, "{}").unwrap();
        let expected_output = directory.join("data.fixed.json");
        let temporary_path = sibling_path(&path, "easy", 0);

        let outcome = write_easy_output_with(
            &path,
            b"complete",
            |file, contents| file.write_all(contents),
            |_path| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected cleanup failure",
                ))
            },
        )
        .unwrap();

        assert_eq!(outcome.path, expected_output);
        assert_eq!(fs::read(&outcome.path).unwrap(), b"complete");
        let warning = outcome.cleanup_warning.unwrap();
        assert_eq!(warning.path, temporary_path);
        assert_eq!(warning.error.kind(), io::ErrorKind::PermissionDenied);

        fs::remove_file(temporary_path).unwrap();
        fs::remove_file(outcome.path).unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn easy_save_error_explains_that_stdout_was_already_printed() {
        let english = easy_save_diagnostic(
            io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            Language::En,
        );
        let chinese = easy_save_diagnostic(
            io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            Language::Zh,
        );

        assert!(english
            .message(Language::En)
            .contains("formatted output was printed, but the new file could not be saved"));
        assert!(chinese
            .message(Language::Zh)
            .contains("格式化结果已显示，但无法保存新文件"));
    }

    #[test]
    fn easy_is_only_a_subcommand_in_the_first_argument_position() {
        let ParsedArgs::Run(options) = parse_args(vec!["--fix".into(), "easy".into()]).unwrap()
        else {
            panic!("expected runnable options");
        };

        assert!(!options.easy);
        assert_eq!(options.input, Some(PathBuf::from("easy")));
    }

    #[test]
    fn parses_cli_flags() {
        let parsed = parse_args(vec![
            "--fix".into(),
            "--stats".into(),
            "--lang".into(),
            "zh".into(),
            "data.json".into(),
        ])
        .unwrap();
        let ParsedArgs::Run(options) = parsed else {
            panic!("expected runnable options");
        };

        assert!(options.fix);
        assert!(options.stats);
        assert_eq!(options.lang, Language::Zh);
        assert_eq!(options.input, Some("data.json".into()));
    }

    #[test]
    fn parses_json5_and_format_flags() {
        let parsed = parse_args(vec![
            "--json5".into(),
            "--indent".into(),
            "4".into(),
            "--color".into(),
            "always".into(),
            "config.json5".into(),
        ])
        .unwrap();
        let ParsedArgs::Run(options) = parsed else {
            panic!("expected runnable options");
        };

        assert_eq!(options.input_mode, InputMode::Json5);
        assert_eq!(options.color, ColorChoice::Always);
        assert_eq!(options.format.indent_width(), 4);
        assert_eq!(options.input, Some("config.json5".into()));
    }

    #[test]
    fn recognizes_help_and_version_without_exiting() {
        assert_eq!(parse_args(vec!["--help".into()]).unwrap(), ParsedArgs::Help);
        assert_eq!(
            parse_args(vec!["--version".into()]).unwrap(),
            ParsedArgs::Version
        );
    }

    #[test]
    fn rejects_conflicting_or_invalid_values() {
        for args in [
            vec!["--color".into()],
            vec!["--color".into(), "sometimes".into()],
            vec!["--compact".into(), "--indent".into(), "4".into()],
            vec!["--check".into(), "--write".into(), "data.json".into()],
            vec!["--write".into()],
        ] {
            assert!(parse_args(args).is_err());
        }
    }

    #[test]
    fn double_dash_allows_option_like_path() {
        let parsed = parse_args(vec!["--".into(), "--data.json".into()]).unwrap();
        let ParsedArgs::Run(options) = parsed else {
            panic!("expected runnable options");
        };

        assert_eq!(options.input, Some("--data.json".into()));
    }

    #[test]
    fn limited_reader_accepts_exact_size_limit() {
        let input = vec![b'a'; MAX_INPUT_BYTES];

        let output = read_limited(Cursor::new(input)).unwrap();

        assert_eq!(output.len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn limited_reader_rejects_oversized_input() {
        let input = vec![b'a'; MAX_INPUT_BYTES + 1];

        let InputError::Content(error) = read_limited(Cursor::new(input)).unwrap_err() else {
            panic!("oversized input must be a content error");
        };

        assert_eq!(
            error.kind,
            DiagnosticKind::InputTooLarge {
                max_bytes: MAX_INPUT_BYTES
            }
        );
    }
    fn temporary_test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("jello-cli-{}-{nonce}-{name}", std::process::id()))
    }

    #[test]
    fn stats_failure_happens_before_a_file_is_replaced() {
        let path = temporary_test_path("stats.json");
        fs::write(&path, "{}").unwrap();
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = BrokenWriter;

        let result = run_with_io(
            vec!["--write".into(), "--stats".into(), path.clone().into()],
            &mut stdin,
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(result, Err(2));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn checked_replacement_refuses_a_concurrent_update() {
        let path = temporary_test_path("concurrent.json");
        fs::write(&path, r#"{"old":1}"#).unwrap();
        let input = read_file_input(&path).unwrap();
        fs::write(&path, r#"{"new":2}"#).unwrap();

        let result = replace_file_safely(
            &path,
            b"{\n  \"old\": 1\n}\n",
            input.source.as_bytes(),
            input.snapshot.as_ref().unwrap(),
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"new":2}"#);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn commit_refuses_a_change_after_temporary_file_creation() {
        let path = temporary_test_path("late-concurrent.json");
        let temporary_path = temporary_test_path("late-concurrent.tmp");
        fs::write(&path, r#"{"old":1}"#).unwrap();
        let input = read_file_input(&path).unwrap();
        fs::write(&temporary_path, b"{\n  \"old\": 1\n}\n").unwrap();
        fs::write(&path, r#"{"new":2}"#).unwrap();

        let result = commit_prepared_replacement(
            &path,
            &temporary_path,
            input.source.as_bytes(),
            input.snapshot.as_ref().unwrap(),
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"new":2}"#);
        assert!(temporary_path.exists());
        fs::remove_file(temporary_path).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rollback_failure_reports_and_preserves_backup_path() {
        let path = temporary_test_path("rollback-target");
        let backup_path = temporary_test_path("rollback-backup");
        let temporary_path = temporary_test_path("rollback-temp");
        fs::write(&backup_path, b"original").unwrap();
        fs::write(&temporary_path, b"replacement").unwrap();
        fs::create_dir(&path).unwrap();

        let error = rollback_failed_replacement(
            &path,
            &backup_path,
            &temporary_path,
            io::Error::new(io::ErrorKind::PermissionDenied, "install failed"),
        );

        let message = error.to_string();
        assert!(message.contains("rollback failed"));
        assert!(message.contains(&backup_path.to_string_lossy().into_owned()));
        assert!(backup_path.exists());
        assert!(temporary_path.exists());
        fs::remove_dir(path).unwrap();
        fs::remove_file(backup_path).unwrap();
        fs::remove_file(temporary_path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn checked_input_refuses_windows_hard_links() {
        let path = temporary_test_path("hard-link-target.json");
        let link = temporary_test_path("hard-link-alias.json");
        fs::write(&path, b"{}").unwrap();
        fs::hard_link(&path, &link).unwrap();

        assert!(read_file_input(&path).is_err());

        fs::remove_file(link).unwrap();
        fs::remove_file(path).unwrap();
    }
}
