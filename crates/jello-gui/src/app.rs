use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText, ScrollArea, TextEdit};

use crate::highlight::{TokenClass, token_spans};
use crate::i18n::{Message, UiLanguage, text};
use crate::model::AppModel;
use crate::schema_engine::SchemaState;
use crate::worker::AnalysisWorker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueTab {
    Problems,
    Repairs,
    Schema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingSave {
    Fixed,
    As,
}

pub fn run(initial_path: Option<OsString>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([760.0, 520.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let initial_path = initial_path.map(PathBuf::from);
    eframe::run_native(
        "Jello",
        options,
        Box::new(move |creation| Ok(Box::new(JelloApp::new(creation, initial_path)))),
    )
}

struct JelloApp {
    model: AppModel,
    worker: AnalysisWorker,
    issue_tab: IssueTab,
    show_issues: bool,
    pending_cursor_byte: Option<usize>,
    pending_save: Option<PendingSave>,
    dark_mode: bool,
}

impl JelloApp {
    fn new(creation: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        creation
            .egui_ctx
            .set_fonts(crate::fonts::font_definitions());
        creation.egui_ctx.set_visuals(egui::Visuals::light());
        let mut app = Self {
            model: AppModel::default(),
            worker: AnalysisWorker::new(creation.egui_ctx.clone()),
            issue_tab: IssueTab::Problems,
            show_issues: true,
            pending_cursor_byte: None,
            pending_save: None,
            dark_mode: false,
        };
        if let Some(path) = initial_path {
            app.open_path(path);
        }
        app
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json", "json5"])
            .pick_file()
        {
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match jello::read_utf8_file_stable(&path) {
            Ok(source) => {
                self.model.open_source(path, source);
                self.worker.invalidate(self.model.analysis_generation());
            }
            Err(error) => self.set_error(
                format!("Unable to open file: {error}"),
                format!("无法打开文件：{error}"),
            ),
        }
    }

    fn schema_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON Schema", &["json", "schema.json"])
            .pick_file()
        {
            self.model.select_schema(path);
            self.worker.invalidate(self.model.analysis_generation());
            self.issue_tab = IssueTab::Schema;
            self.show_issues = true;
        }
    }

    fn save_fixed(&mut self) {
        let Some(source_path) = self.model.source_path.as_deref() else {
            return;
        };
        let Some(preview) = self.model.preview.as_deref() else {
            return;
        };
        let bytes = output_bytes(preview);
        match jello::save_fixed(source_path, &bytes) {
            Ok(saved) => self.report_saved(saved),
            Err(error) => self.set_error(
                format!("Unable to save formatted output: {error}"),
                format!("无法保存格式化结果：{error}"),
            ),
        }
    }

    fn save_as(&mut self) {
        let Some(source_path) = self.model.source_path.as_deref() else {
            return;
        };
        let Some(preview) = self.model.preview.as_deref() else {
            return;
        };
        let suggested = suggested_output_name(source_path);
        let Some(destination) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(suggested)
            .save_file()
        else {
            return;
        };
        let bytes = output_bytes(preview);
        match jello::save_as_new(source_path, &destination, &bytes) {
            Ok(saved) => self.report_saved(saved),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => self.set_error(
                "That file already exists. Jello never overwrites files.".to_string(),
                "该文件已经存在。Jello 不会覆盖任何文件。".to_string(),
            ),
            Err(error) => self.set_error(
                format!("Unable to save formatted output: {error}"),
                format!("无法保存格式化结果：{error}"),
            ),
        }
    }

    fn report_saved(&mut self, saved: jello::SavedOutput) {
        let path = saved.path.to_string_lossy();
        let mut message = match self.model.language {
            UiLanguage::En => format!("Saved formatted output to {path}"),
            UiLanguage::Zh => format!("格式化结果已保存到 {path}"),
        };
        if let Some(warning) = saved.cleanup_warning {
            match self.model.language {
                UiLanguage::En => {
                    message.push_str(&format!(
                        "; temporary file cleanup failed at {}: {}",
                        warning.path.to_string_lossy(),
                        warning.error
                    ));
                }
                UiLanguage::Zh => {
                    message.push_str(&format!(
                        "；临时文件 {} 清理失败：{}",
                        warning.path.to_string_lossy(),
                        warning.error
                    ));
                }
            }
        }
        self.model.status = Some(message);
    }

    fn set_error(&mut self, english: String, chinese: String) {
        self.model.status = Some(match self.model.language {
            UiLanguage::En => english,
            UiLanguage::Zh => chinese,
        });
    }

    fn handle_dropped_file(&mut self, context: &egui::Context) {
        let paths = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        let (path, ignored) = select_dropped_path(paths);
        if let Some(path) = path {
            self.open_path(path);
            if ignored > 0 {
                self.model.status = Some(match self.model.language {
                    UiLanguage::En => format!(
                        "Opened the first file and ignored {ignored} more; use jello-drop.exe for batch processing."
                    ),
                    UiLanguage::Zh => format!(
                        "已打开第一个文件，另有 {ignored} 个文件未处理；批量处理请使用 jello-drop.exe。"
                    ),
                });
            }
        }
    }
    fn poll_analysis(&mut self, context: &egui::Context) {
        while let Ok(result) = self.worker.try_recv() {
            self.model.apply_result(result);
        }
        if let Some(request) = self.model.take_analysis_request(Instant::now())
            && self.worker.submit(request).is_err()
        {
            self.set_error(
                "The analysis worker stopped unexpectedly.".to_string(),
                "分析线程意外停止。".to_string(),
            );
        }
        context.request_repaint_after(Duration::from_millis(50));
    }

    fn toolbar(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button(text(self.model.language, Message::OpenJson))
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .button(text(self.model.language, Message::LoadSchema))
                .clicked()
            {
                self.schema_dialog();
            }

            egui::ComboBox::from_id_salt("language")
                .selected_text(match self.model.language {
                    UiLanguage::En => "English",
                    UiLanguage::Zh => "中文",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.model.language, UiLanguage::En, "English");
                    ui.selectable_value(&mut self.model.language, UiLanguage::Zh, "中文");
                });

            let theme_label = if self.dark_mode {
                text(self.model.language, Message::Light)
            } else {
                text(self.model.language, Message::Dark)
            };
            if ui.button(theme_label).clicked() {
                self.dark_mode = !self.dark_mode;
                context.set_visuals(if self.dark_mode {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
            }

            ui.separator();
            let source_name = self
                .model
                .source_path
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| "—".into());
            ui.label(RichText::new(source_name).weak());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_save = self.model.can_save();
                if ui
                    .add_enabled(
                        can_save,
                        egui::Button::new(text(self.model.language, Message::SaveFixed)),
                    )
                    .clicked()
                {
                    self.pending_save = Some(PendingSave::Fixed);
                }
                if ui
                    .add_enabled(
                        can_save,
                        egui::Button::new(text(self.model.language, Message::SaveAs)),
                    )
                    .clicked()
                {
                    self.pending_save = Some(PendingSave::As);
                }
            });
        });
    }

    fn source_pane(&mut self, ui: &mut egui::Ui) {
        pane_header(
            ui,
            text(self.model.language, Message::Source),
            self.model
                .source_path
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .as_deref(),
        );
        if self.model.source_path.is_none() && self.model.source.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(text(self.model.language, Message::DropHint)).weak());
            });
            return;
        }

        let mut changed = false;
        let cursor_byte = self.pending_cursor_byte.take();
        let desired_rows = self.model.source.lines().count().max(24);
        ScrollArea::both()
            .id_salt("source-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    line_numbers(ui, &self.model.source);
                    let dark = ui.visuals().dark_mode;
                    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                        let job = highlight_job(buffer.as_str(), dark);
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    let mut output = TextEdit::multiline(&mut self.model.source)
                        .id_salt("source-editor")
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(desired_rows)
                        .layouter(&mut layouter)
                        .show(ui);
                    changed = output.response.changed();
                    if let Some(byte) = cursor_byte {
                        let index = byte_to_char_index(&self.model.source, byte);
                        let cursor = CCursor::new(index);
                        output
                            .state
                            .cursor
                            .set_char_range(Some(CCursorRange::two(cursor, cursor)));
                        output.state.store(ui.ctx(), output.response.response.id);
                        output.response.request_focus();
                        output.response.scroll_to_me(Some(egui::Align::Center));
                    }
                });
            });
        if changed {
            self.model.mark_edited(Instant::now());
            self.worker.invalidate(self.model.analysis_generation());
        }
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui) {
        let preview_state = self
            .model
            .preview
            .as_ref()
            .map(|_| format!("● {}", text(self.model.language, Message::Live)));
        pane_header(
            ui,
            text(self.model.language, Message::Preview),
            preview_state.as_deref(),
        );
        let mut preview = self.model.preview.clone().unwrap_or_default();
        if preview.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("—").weak());
            });
            return;
        }
        let desired_rows = preview.lines().count().max(24);
        ScrollArea::both()
            .id_salt("preview-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    line_numbers(ui, &preview);
                    let dark = ui.visuals().dark_mode;
                    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                        let job = highlight_job(buffer.as_str(), dark);
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    ui.add(
                        TextEdit::multiline(&mut preview)
                            .id_salt("preview-editor")
                            .code_editor()
                            .interactive(false)
                            .desired_width(f32::INFINITY)
                            .desired_rows(desired_rows)
                            .layouter(&mut layouter),
                    );
                });
            });
    }

    fn issues_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .small_button(if self.show_issues { "▼" } else { "▶" })
                .clicked()
            {
                self.show_issues = !self.show_issues;
            }
            if ui
                .selectable_label(
                    self.issue_tab == IssueTab::Problems,
                    format!(
                        "{} {}",
                        text(self.model.language, Message::Problems),
                        self.model.diagnostics.len()
                    ),
                )
                .clicked()
            {
                self.issue_tab = IssueTab::Problems;
                self.show_issues = true;
            }
            if ui
                .selectable_label(
                    self.issue_tab == IssueTab::Repairs,
                    format!(
                        "{} {}",
                        text(self.model.language, Message::Repairs),
                        self.model.repairs.len()
                    ),
                )
                .clicked()
            {
                self.issue_tab = IssueTab::Repairs;
                self.show_issues = true;
            }
            if ui
                .selectable_label(
                    self.issue_tab == IssueTab::Schema,
                    text(self.model.language, Message::Schema),
                )
                .clicked()
            {
                self.issue_tab = IssueTab::Schema;
                self.show_issues = true;
            }
        });
        if !self.show_issues {
            return;
        }
        ui.separator();
        ScrollArea::vertical().show(ui, |ui| match self.issue_tab {
            IssueTab::Problems => self.problem_rows(ui),
            IssueTab::Repairs => self.repair_rows(ui),
            IssueTab::Schema => self.schema_rows(ui),
        });
    }

    fn problem_rows(&mut self, ui: &mut egui::Ui) {
        if self.model.diagnostics.is_empty() {
            ui.label(text(self.model.language, Message::NoProblems));
            return;
        }
        for diagnostic in &self.model.diagnostics {
            let language = match self.model.language {
                UiLanguage::En => jello::Language::En,
                UiLanguage::Zh => jello::Language::Zh,
            };
            let location = diagnostic
                .span
                .map(|span| format!("{}:{}", span.start.line, span.start.column))
                .unwrap_or_default();
            if ui
                .selectable_label(
                    false,
                    format!(
                        "{}  {}  {}",
                        diagnostic.code,
                        diagnostic.message(language),
                        location
                    ),
                )
                .clicked()
            {
                self.pending_cursor_byte = diagnostic.span.map(|span| span.start.byte);
            }
        }
    }

    fn repair_rows(&mut self, ui: &mut egui::Ui) {
        if self.model.repairs.is_empty() {
            ui.label(text(self.model.language, Message::NoRepairs));
            return;
        }
        for repair in &self.model.repairs {
            ui.label(format!(
                "{}  {}  {}:{}",
                repair.code,
                repair_description(repair.code, &repair.description, self.model.language),
                repair.line,
                repair.column
            ));
        }
    }

    fn schema_rows(&mut self, ui: &mut egui::Ui) {
        if let Some(path) = &self.model.schema_path {
            ui.label(RichText::new(path.to_string_lossy()).strong());
            ui.separator();
        }
        match &self.model.schema_state {
            SchemaState::NotLoaded => {
                ui.label(text(self.model.language, Message::NoSchema));
            }
            SchemaState::Valid => {
                ui.colored_label(
                    Color32::from_rgb(22, 130, 93),
                    text(self.model.language, Message::SchemaValid),
                );
            }
            SchemaState::Invalid(issues) => {
                for issue in issues {
                    ui.colored_label(
                        Color32::from_rgb(190, 50, 45),
                        format!(
                            "{}  {}  {}",
                            issue.instance_path, issue.message, issue.schema_path
                        ),
                    );
                }
            }
            SchemaState::LoadError(error) => {
                ui.colored_label(Color32::from_rgb(190, 50, 45), error);
            }
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(status) = &self.model.status {
                ui.label(status);
            } else if self.model.preview.is_some() {
                ui.colored_label(
                    Color32::from_rgb(22, 130, 93),
                    format!("● {}", text(self.model.language, Message::PreviewReady)),
                );
            } else {
                ui.label(format!("● {}", text(self.model.language, Message::Waiting)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(text(self.model.language, Message::OriginalProtected));
            });
        });
    }
}

impl eframe::App for JelloApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_dropped_file(context);
        self.poll_analysis(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        egui::Panel::top("toolbar").show(ui, |ui| {
            self.toolbar(&context, ui);
        });
        egui::Panel::bottom("status")
            .exact_size(28.0)
            .show(ui, |ui| self.status_bar(ui));
        let issue_height = if self.show_issues { 180.0 } else { 34.0 };
        egui::Panel::bottom("issues")
            .resizable(self.show_issues)
            .default_size(issue_height)
            .min_size(34.0)
            .show(ui, |ui| self.issues_panel(ui));
        egui::Panel::left("source")
            .resizable(true)
            .default_size(570.0)
            .min_size(300.0)
            .show(ui, |ui| self.source_pane(ui));
        egui::CentralPanel::default().show(ui, |ui| self.preview_pane(ui));
        if let Some(save) = self.pending_save.take() {
            match save {
                PendingSave::Fixed => self.save_fixed(),
                PendingSave::As => self.save_as(),
            }
        }
    }
}

fn pane_header(ui: &mut egui::Ui, title: &str, detail: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().small());
        if let Some(detail) = detail {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(detail).weak().small());
            });
        }
    });
    ui.separator();
}

fn line_numbers(ui: &mut egui::Ui, source: &str) {
    let count = source.lines().count().max(1);
    let width = count.to_string().len();
    let mut numbers = String::new();
    for line in 1..=count {
        numbers.push_str(&format!("{line:>width$}\n"));
    }
    ui.label(
        RichText::new(numbers)
            .monospace()
            .color(ui.visuals().weak_text_color()),
    );
}

fn highlight_job(source: &str, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for span in token_spans(source) {
        let color = match (dark, span.class) {
            (_, TokenClass::Plain) => Color32::GRAY,
            (false, TokenClass::Punctuation) => Color32::from_rgb(55, 65, 81),
            (true, TokenClass::Punctuation) => Color32::from_rgb(210, 215, 225),
            (false, TokenClass::String) => Color32::from_rgb(23, 107, 77),
            (true, TokenClass::String) => Color32::from_rgb(120, 205, 155),
            (false, TokenClass::Number) => Color32::from_rgb(162, 75, 25),
            (true, TokenClass::Number) => Color32::from_rgb(240, 170, 105),
            (false, TokenClass::Keyword) => Color32::from_rgb(37, 99, 235),
            (true, TokenClass::Keyword) => Color32::from_rgb(120, 165, 255),
            (false, TokenClass::Identifier) => Color32::from_rgb(126, 65, 155),
            (true, TokenClass::Identifier) => Color32::from_rgb(205, 150, 235),
            (_, TokenClass::Comment) => Color32::from_rgb(105, 125, 110),
            (_, TokenClass::Invalid) => Color32::from_rgb(220, 50, 47),
        };
        job.append(
            &source[span.range],
            0.0,
            TextFormat {
                font_id: FontId::monospace(14.0),
                color,
                ..Default::default()
            },
        );
    }
    job
}

fn output_bytes(preview: &str) -> Vec<u8> {
    let mut bytes = preview.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes
}

fn select_dropped_path(paths: Vec<PathBuf>) -> (Option<PathBuf>, usize) {
    let ignored = paths.len().saturating_sub(1);
    (paths.into_iter().next(), ignored)
}
fn suggested_output_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .or_else(|| source.file_name())
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "output".into());
    format!("{stem}.fixed.json")
}

fn repair_description<'a>(code: &str, fallback: &'a str, language: UiLanguage) -> &'a str {
    if language == UiLanguage::En {
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

pub(crate) fn byte_to_char_index(source: &str, byte: usize) -> usize {
    let mut end = byte.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    source[..end].chars().count()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{byte_to_char_index, select_dropped_path};

    #[test]
    fn multiple_drops_open_only_the_first_and_report_the_rest() {
        let (selected, ignored) = select_dropped_path(vec![
            PathBuf::from("first.json"),
            PathBuf::from("second.json"),
            PathBuf::from("third.json"),
        ]);

        assert_eq!(selected, Some(PathBuf::from("first.json")));
        assert_eq!(ignored, 2);
    }

    #[test]
    fn diagnostic_byte_offsets_convert_to_egui_character_offsets() {
        let source = "a中🦀b";

        assert_eq!(byte_to_char_index(source, 0), 0);
        assert_eq!(byte_to_char_index(source, 1), 1);
        assert_eq!(byte_to_char_index(source, 4), 2);
        assert_eq!(byte_to_char_index(source, 8), 3);
        assert_eq!(byte_to_char_index(source, usize::MAX), 4);
    }
}
