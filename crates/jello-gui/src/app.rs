use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::text::{CCursor, CCursorRange};
use egui::{Color32, RichText, ScrollArea, TextEdit};
use jello::{RepairCandidate, RepairDecision, RepairEvaluation};

use crate::highlight::{OverlayRange, OverlayState, TokenCache, layout_job_with_spans};
use crate::i18n::{Message, UiLanguage, text};
use crate::model::AppModel;
use crate::schema_engine::SchemaState;
use crate::worker::AnalysisWorker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueTab {
    Problems,
    Repairs,
    Changes,
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
    last_candidate: Option<RepairCandidate>,
    preview_text: String,
    source_tokens: TokenCache,
    preview_tokens: TokenCache,
    saved_language: UiLanguage,
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
            last_candidate: None,
            preview_text: String::new(),
            source_tokens: TokenCache::default(),
            preview_tokens: TokenCache::default(),
            saved_language: UiLanguage::En,
        };
        if let Some(path) = language_config_path()
            && let Ok(Some(language)) = jello::load_language_config(&path)
        {
            app.model.language = match language {
                jello::Language::En => UiLanguage::En,
                jello::Language::Zh => UiLanguage::Zh,
            };
            app.saved_language = app.model.language;
        }
        if let Some(path) = initial_path {
            let _ = app.open_path(path);
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

    fn open_path(&mut self, path: PathBuf) -> bool {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                self.set_error(
                    "Symbolic links are not accepted as GUI input.".to_string(),
                    "GUI 不接受符号链接作为输入。".to_string(),
                );
                return false;
            }
            Ok(metadata) if !metadata.file_type().is_file() => {
                self.set_error(
                    "Please open a regular JSON or JSON5 file.".to_string(),
                    "请打开普通的 JSON 或 JSON5 文件。".to_string(),
                );
                return false;
            }
            Err(error) => {
                self.set_error(
                    format!("Unable to inspect file: {error}"),
                    format!("无法检查文件：{error}"),
                );
                return false;
            }
            Ok(_) => {}
        }
        match jello::read_utf8_file_stable(&path) {
            Ok(source) => {
                self.model.open_source(path, source);
                self.last_candidate = None;
                self.preview_text.clear();
                self.worker.invalidate(self.model.analysis_generation());
                true
            }
            Err(error) => {
                self.set_error(
                    format!("Unable to open file: {error}"),
                    format!("无法打开文件：{error}"),
                );
                false
            }
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
        let Some(preview) = self.model.preview.as_deref() else {
            return;
        };
        if let Err(error) = self.model.verify_save_target() {
            self.set_error(
                format!("Unable to save: {error}"),
                format!("无法保存：{error}"),
            );
            return;
        }
        let bytes = output_bytes(preview);
        if let Some(path) = self.model.saved_path.clone() {
            let expected = self.model.saved_snapshot.clone().unwrap_or_default();
            match jello::save_updated(&path, &expected, &bytes) {
                Ok(()) => {
                    self.model.saved_snapshot = Some(bytes);
                    self.report_saved(jello::SavedOutput {
                        path,
                        cleanup_warning: None,
                    });
                }
                Err(error) => self.set_error(
                    format!("Unable to update saved output: {error}"),
                    format!("无法更新已保存的输出：{error}"),
                ),
            }
            return;
        }
        let Some(source_path) = self.model.source_path.clone() else {
            return;
        };
        match jello::save_fixed(&source_path, &bytes) {
            Ok(saved) => {
                self.model.saved_path = Some(saved.path.clone());
                self.model.saved_snapshot = Some(bytes);
                self.report_saved(saved);
            }
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
        if let Err(error) = self.model.verify_source_unchanged() {
            self.set_error(
                format!("Unable to save: {error}"),
                format!("无法保存：{error}"),
            );
            return;
        }
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
            Ok(saved) => {
                self.model.saved_path = Some(saved.path.clone());
                self.model.saved_snapshot = Some(bytes);
                self.report_saved(saved);
            }
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
        if let Some(path) = path
            && self.open_path(path)
            && ignored > 0
        {
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
    fn poll_analysis(&mut self, context: &egui::Context) {
        let mut applied = false;
        while let Ok(result) = self.worker.try_recv() {
            let candidate = result
                .evaluation
                .as_ref()
                .and_then(evaluation_candidate)
                .cloned();
            let applied_now = self.model.apply_result(result);
            if applied_now && let Some(candidate) = candidate {
                self.last_candidate = Some(candidate);
            }
            applied |= applied_now;
        }
        if applied {
            self.refresh_preview_text();
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

    fn refresh_preview_text(&mut self) {
        self.preview_text = match &self.model.review {
            Some(review) => match review.evaluation() {
                RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
                    candidate.output.clone()
                }
                RepairEvaluation::Invalid { .. } => self
                    .last_candidate
                    .as_ref()
                    .map(|candidate| candidate.output.clone())
                    .unwrap_or_default(),
            },
            None => self.model.preview.clone().unwrap_or_default(),
        };
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
            if self.model.language != self.saved_language {
                self.saved_language = self.model.language;
                if let Some(path) = language_config_path() {
                    let language = match self.model.language {
                        UiLanguage::En => jello::Language::En,
                        UiLanguage::Zh => jello::Language::Zh,
                    };
                    if let Err(error) = jello::save_language_config(&path, language) {
                        self.set_error(
                            format!("Unable to save language setting: {error}"),
                            format!("无法保存语言设置：{error}"),
                        );
                    }
                }
            }

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
                        egui::Button::new(text(
                            self.model.language,
                            if self.model.saved_path.is_some() {
                                Message::Save
                            } else {
                                Message::SaveFixed
                            },
                        )),
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
        let overlays = self
            .model
            .review
            .as_ref()
            .map(source_overlays)
            .unwrap_or_default();
        let desired_rows = self.model.source.lines().count().max(24);
        let source = &mut self.model.source;
        let tokens = &mut self.source_tokens;
        ScrollArea::both()
            .id_salt("source-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    line_numbers(ui, source);
                    let dark = ui.visuals().dark_mode;
                    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                        let text = buffer.as_str();
                        let spans = tokens.spans_for(text);
                        let job = layout_job_with_spans(text, spans, dark, &overlays);
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    let mut output = TextEdit::multiline(source)
                        .id_salt("source-editor")
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(desired_rows)
                        .layouter(&mut layouter)
                        .show(ui);
                    changed = output.response.changed();
                    if let Some(byte) = cursor_byte {
                        let index = byte_to_char_index(source, byte);
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
            self.last_candidate = None;
            self.preview_text.clear();
            self.worker.invalidate(self.model.analysis_generation());
        }
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui) {
        let (overlays, stale, preview_state) = self.preview_content();
        let can_copy = !self.preview_text.is_empty();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(text(self.model.language, Message::Preview))
                    .strong()
                    .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        can_copy,
                        egui::Button::new(text(self.model.language, Message::Copy)),
                    )
                    .clicked()
                {
                    self.copy_preview();
                }
                if let Some(state) = preview_state {
                    ui.label(RichText::new(state).weak().small());
                }
            });
        });
        ui.separator();
        if self.preview_text.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("—").weak());
            });
            return;
        }
        let preview = &mut self.preview_text;
        let desired_rows = preview.lines().count().max(24);
        ScrollArea::both()
            .id_salt("preview-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    line_numbers(ui, preview);
                    let dark = ui.visuals().dark_mode;
                    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                        let text = buffer.as_str();
                        let spans = self.preview_tokens.spans_for(text);
                        let mut job = layout_job_with_spans(text, spans, dark, &overlays);
                        if stale {
                            for section in &mut job.sections {
                                section.format.color = section.format.color.gamma_multiply(0.65);
                            }
                        }
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    ui.add(
                        TextEdit::multiline(&mut *preview)
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

    fn copy_preview(&mut self) {
        if self.preview_text.is_empty() {
            return;
        }
        match arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(self.preview_text.clone()))
        {
            Ok(()) => {
                self.model.status = Some(text(self.model.language, Message::Copied).to_string());
            }
            Err(error) => self.set_error(
                format!("Unable to copy to clipboard: {error}"),
                format!("无法复制到剪贴板：{error}"),
            ),
        }
    }

    fn preview_content(&self) -> (Vec<OverlayRange>, bool, Option<String>) {
        let Some(review) = self.model.review.as_ref() else {
            return (
                Vec::new(),
                false,
                self.model
                    .preview
                    .as_ref()
                    .map(|_| text(self.model.language, Message::Live).to_string()),
            );
        };
        match review.evaluation() {
            RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => (
                candidate_overlays(review, candidate),
                false,
                Some(text(self.model.language, Message::Live).to_string()),
            ),
            RepairEvaluation::Invalid { .. } => {
                let Some(candidate) = self.last_candidate.as_ref() else {
                    return (
                        Vec::new(),
                        false,
                        Some(text(self.model.language, Message::Invalid).to_string()),
                    );
                };
                (
                    candidate_overlays(review, candidate),
                    true,
                    Some(text(self.model.language, Message::StalePreview).to_string()),
                )
            }
        }
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
                        self.model
                            .review
                            .as_ref()
                            .map_or(0, |review| review.plan().groups().len())
                    ),
                )
                .clicked()
            {
                self.issue_tab = IssueTab::Repairs;
                self.show_issues = true;
            }
            if ui
                .selectable_label(
                    self.issue_tab == IssueTab::Changes,
                    text(self.model.language, Message::Changes),
                )
                .clicked()
            {
                self.issue_tab = IssueTab::Changes;
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
            IssueTab::Changes => self.changes_rows(ui),
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
        let Some(review) = self.model.review.as_ref() else {
            ui.label(text(self.model.language, Message::NoRepairs));
            return;
        };
        if review.plan().groups().is_empty() {
            ui.label(text(self.model.language, Message::NoRepairs));
            return;
        }
        let pending = review.pending_count();
        let selected_group = review.selected_group();
        let rows: Vec<_> = review
            .plan()
            .groups()
            .iter()
            .map(|repair| {
                let first_change = repair.changes().first();
                (
                    repair.id(),
                    repair.decision_set(),
                    review.selection().decision(repair.decision_set()),
                    repair.code(),
                    repair.description(),
                    first_change.map_or(0, |change| change.span().start.line),
                    first_change.map_or(0, |change| change.span().start.column),
                    first_change.map(|change| change.byte_range().start),
                )
            })
            .collect();
        ui.horizontal(|ui| {
            if ui
                .button(text(self.model.language, Message::AcceptAll))
                .clicked()
            {
                self.model.set_all_repairs(RepairDecision::Accepted);
            }
            if ui
                .button(text(self.model.language, Message::RejectAll))
                .clicked()
            {
                self.model.set_all_repairs(RepairDecision::Rejected);
            }
            ui.label(format!(
                "{}: {pending}",
                text(self.model.language, Message::Pending)
            ));
        });
        for (id, decision_set, decision, code, description, line, column, start_byte) in rows {
            ui.horizontal(|ui| {
                let is_selected = selected_group == Some(id);
                if ui
                    .selectable_label(
                        is_selected,
                        format!(
                            "{}  {}  {}:{}",
                            code,
                            repair_description(code, description, self.model.language),
                            line,
                            column
                        ),
                    )
                    .clicked()
                {
                    if is_selected {
                        self.model.select_repair(None);
                    } else {
                        self.pending_cursor_byte = start_byte;
                        self.model.select_repair(Some(id));
                    }
                }
                if ui
                    .selectable_label(
                        decision == Some(RepairDecision::Accepted),
                        text(self.model.language, Message::Accept),
                    )
                    .clicked()
                {
                    self.model
                        .decide_repair(decision_set, RepairDecision::Accepted);
                }
                if ui
                    .selectable_label(
                        decision == Some(RepairDecision::Rejected),
                        text(self.model.language, Message::Reject),
                    )
                    .clicked()
                {
                    self.model
                        .decide_repair(decision_set, RepairDecision::Rejected);
                }
            });
        }
    }

    fn changes_rows(&mut self, ui: &mut egui::Ui) {
        if self.preview_text.is_empty() {
            ui.label(RichText::new("—").weak());
            return;
        }
        let Some(ops) = jello::diff_lines(&self.model.source, &self.preview_text) else {
            ui.label(text(self.model.language, Message::DiffSkipped));
            return;
        };
        if ops.is_empty() {
            ui.label(text(self.model.language, Message::NoChanges));
            return;
        }
        let before_lines: Vec<&str> = self.model.source.lines().collect();
        let after_lines: Vec<&str> = self.preview_text.lines().collect();
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        ScrollArea::vertical().show_rows(ui, row_height, ops.len(), |ui, range| {
            for index in range {
                ui.horizontal(|ui| match &ops[index] {
                    jello::DiffOp::Equal { before, after } => {
                        ui.monospace(format!("{:>5} {:>5} │", before + 1, after + 1));
                        ui.monospace(before_lines[*before]);
                    }
                    jello::DiffOp::Delete { before } => {
                        ui.monospace(format!("{:>5}       │", before + 1));
                        ui.monospace(
                            RichText::new(format!("- {}", before_lines[*before]))
                                .background_color(Color32::from_rgba_unmultiplied(239, 68, 68, 40)),
                        );
                    }
                    jello::DiffOp::Insert { after } => {
                        ui.monospace(format!("       {:>5} │", after + 1));
                        ui.monospace(
                            RichText::new(format!("+ {}", after_lines[*after]))
                                .background_color(Color32::from_rgba_unmultiplied(34, 197, 94, 40)),
                        );
                    }
                });
            }
        });
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
            } else if self.model.can_save() {
                ui.colored_label(
                    Color32::from_rgb(22, 130, 93),
                    format!("● {}", text(self.model.language, Message::PreviewReady)),
                );
            } else if self.model.analysis_in_flight() {
                ui.label(format!(
                    "● {}",
                    text(self.model.language, Message::Analyzing)
                ));
            } else if self.model.review.is_some() {
                ui.label(format!(
                    "● {}",
                    text(self.model.language, Message::NeedsReview)
                ));
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

fn evaluation_candidate(evaluation: &RepairEvaluation) -> Option<&RepairCandidate> {
    match evaluation {
        RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
            Some(candidate)
        }
        RepairEvaluation::Invalid { .. } => None,
    }
}

fn source_overlays(review: &crate::review::ReviewState) -> Vec<OverlayRange> {
    let selected = review.selected_group();
    review
        .plan()
        .groups()
        .iter()
        .flat_map(|group| {
            let state = overlay_state(
                review
                    .selection()
                    .decision(group.decision_set())
                    .unwrap_or(RepairDecision::Pending),
            );
            group.changes().iter().map(move |change| OverlayRange {
                range: change.byte_range(),
                state,
                selected: selected == Some(group.id()),
            })
        })
        .collect()
}

fn candidate_overlays(
    review: &crate::review::ReviewState,
    candidate: &RepairCandidate,
) -> Vec<OverlayRange> {
    // Highlights normally come from the same analysis result as the review
    // plan, but a stale candidate (for example from an earlier plan) must
    // never index past the plan's groups.
    let selected = review.selected_group();
    candidate
        .highlights
        .iter()
        .filter_map(|highlight| {
            let group = review.plan().groups().get(highlight.group.index())?;
            let decision = review
                .selection()
                .decision(group.decision_set())
                .unwrap_or(RepairDecision::Pending);
            (decision == RepairDecision::Pending).then(|| OverlayRange {
                range: highlight.range.clone(),
                state: OverlayState::Changed,
                selected: selected == Some(highlight.group),
            })
        })
        .collect()
}

fn overlay_state(decision: RepairDecision) -> OverlayState {
    match decision {
        RepairDecision::Pending => OverlayState::Pending,
        RepairDecision::Accepted => OverlayState::Accepted,
        RepairDecision::Rejected => OverlayState::Rejected,
    }
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

fn language_config_path() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local_app_data).join("Jello").join("config"))
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
    use std::sync::Arc;

    use jello::{RepairDecision, RepairEvaluation};

    use crate::highlight::OverlayState;
    use crate::review::ReviewState;

    use super::{byte_to_char_index, candidate_overlays, select_dropped_path, source_overlays};

    #[test]
    fn review_overlay_builders_use_group_changes_and_candidate_highlights() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let review = ReviewState::new(plan.clone(), selection, evaluation);
        let candidate = match review.evaluation() {
            RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => candidate,
            RepairEvaluation::Invalid { .. } => panic!("repairable source must yield a candidate"),
        };

        let source = source_overlays(&review);
        assert_eq!(
            source
                .iter()
                .map(|overlay| overlay.range.clone())
                .collect::<Vec<_>>(),
            plan.groups()
                .iter()
                .flat_map(|group| group.changes().iter().map(|change| change.byte_range()))
                .collect::<Vec<_>>(),
        );
        assert!(
            source
                .iter()
                .all(|overlay| overlay.state == OverlayState::Pending)
        );

        let preview = candidate_overlays(&review, candidate);
        assert_eq!(
            preview
                .iter()
                .map(|overlay| overlay.range.clone())
                .collect::<Vec<_>>(),
            candidate
                .highlights
                .iter()
                .map(|highlight| highlight.range.clone())
                .collect::<Vec<_>>(),
        );
        assert!(
            preview
                .iter()
                .all(|overlay| overlay.state == OverlayState::Changed)
        );
    }

    #[test]
    fn preview_highlights_only_repairs_that_still_await_a_decision() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let mut review = ReviewState::new(plan.clone(), selection, evaluation);

        let pending = candidate_overlays(
            &review,
            match review.evaluation() {
                RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
                    candidate
                }
                RepairEvaluation::Invalid { .. } => {
                    panic!("repairable source must yield a candidate")
                }
            },
        );
        assert_eq!(pending.len(), review.plan().groups().len());

        review.set_all(RepairDecision::Accepted);
        review.replace_evaluation(review.plan().evaluate(review.selection()));
        let accepted = candidate_overlays(
            &review,
            match review.evaluation() {
                RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
                    candidate
                }
                RepairEvaluation::Invalid { .. } => panic!("accepted plan must stay evaluable"),
            },
        );
        assert!(
            accepted.is_empty(),
            "accepted repairs are the final result and must not be highlighted"
        );
    }

    #[test]
    fn stale_candidate_highlights_outside_the_plan_are_ignored() {
        let old_plan = Arc::new(jello::plan_repair_json5("{name:'Ada', city:'BJ'}").unwrap());
        let candidate = match old_plan.evaluate(&old_plan.default_selection()) {
            RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => candidate,
            RepairEvaluation::Invalid { .. } => panic!("repairable source must yield a candidate"),
        };
        assert!(candidate.highlights.len() > 1);

        let new_plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let review = ReviewState::new(
            new_plan.clone(),
            new_plan.default_selection(),
            new_plan.evaluate(&new_plan.default_selection()),
        );

        let overlays = candidate_overlays(&review, &candidate);

        assert!(
            overlays.len() == new_plan.groups().len(),
            "out-of-plan highlights must be skipped instead of panicking"
        );
    }

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
