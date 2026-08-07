#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLanguage {
    #[default]
    En,
    Zh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    OpenJson,
    LoadSchema,
    SaveAs,
    SaveFixed,
    Save,
    AcceptAll,
    RejectAll,
    Accept,
    Reject,
    Invalid,
    StalePreview,
    Pending,
    Source,
    Preview,
    Copy,
    Copied,
    Problems,
    Repairs,
    Changes,
    Schema,
    NoProblems,
    NoRepairs,
    NoChanges,
    DiffSkipped,
    NoSchema,
    SchemaValid,
    OriginalProtected,
    DropHint,
    Light,
    Dark,
    Live,
    PreviewReady,
    NeedsReview,
    Analyzing,
    Waiting,
}

pub fn text(language: UiLanguage, message: Message) -> &'static str {
    match (language, message) {
        (UiLanguage::En, Message::OpenJson) => "Open JSON",
        (UiLanguage::Zh, Message::OpenJson) => "打开 JSON",
        (UiLanguage::En, Message::LoadSchema) => "Load Schema",
        (UiLanguage::Zh, Message::LoadSchema) => "加载 Schema",
        (UiLanguage::En, Message::SaveAs) => "Save As…",
        (UiLanguage::Zh, Message::SaveAs) => "另存为…",
        (UiLanguage::En, Message::SaveFixed) => "Save .fixed",
        (UiLanguage::Zh, Message::SaveFixed) => "保存 .fixed",
        (UiLanguage::En, Message::Save) => "Save",
        (UiLanguage::Zh, Message::Save) => "保存",
        (UiLanguage::En, Message::AcceptAll) => "Accept all",
        (UiLanguage::Zh, Message::AcceptAll) => "全部接受",
        (UiLanguage::En, Message::RejectAll) => "Reject all",
        (UiLanguage::Zh, Message::RejectAll) => "全部拒绝",
        (UiLanguage::En, Message::Accept) => "Accept",
        (UiLanguage::Zh, Message::Accept) => "接受",
        (UiLanguage::En, Message::Reject) => "Reject",
        (UiLanguage::Zh, Message::Reject) => "拒绝",
        (UiLanguage::En, Message::Invalid) => "Invalid",
        (UiLanguage::Zh, Message::Invalid) => "无效",
        (UiLanguage::En, Message::StalePreview) => "Invalid (stale preview)",
        (UiLanguage::Zh, Message::StalePreview) => "无效（预览已过期）",
        (UiLanguage::En, Message::Pending) => "Pending",
        (UiLanguage::Zh, Message::Pending) => "待处理",
        (UiLanguage::En, Message::Source) => "EDITABLE SOURCE",
        (UiLanguage::Zh, Message::Source) => "可编辑源文件",
        (UiLanguage::En, Message::Preview) => "FORMATTED PREVIEW",
        (UiLanguage::Zh, Message::Preview) => "格式化预览",
        (UiLanguage::En, Message::Copy) => "Copy",
        (UiLanguage::Zh, Message::Copy) => "复制",
        (UiLanguage::En, Message::Copied) => "Copied formatted output to clipboard",
        (UiLanguage::Zh, Message::Copied) => "格式化结果已复制到剪贴板",
        (UiLanguage::En, Message::Problems) => "Problems",
        (UiLanguage::Zh, Message::Problems) => "问题",
        (UiLanguage::En, Message::Repairs) => "Repairs",
        (UiLanguage::Zh, Message::Repairs) => "修复记录",
        (UiLanguage::En, Message::Changes) => "Changes",
        (UiLanguage::Zh, Message::Changes) => "变更",
        (UiLanguage::En, Message::Schema) => "Schema",
        (UiLanguage::Zh, Message::Schema) => "Schema",
        (UiLanguage::En, Message::NoProblems) => "No syntax problems.",
        (UiLanguage::Zh, Message::NoProblems) => "没有语法问题。",
        (UiLanguage::En, Message::NoRepairs) => "No repair edits were needed.",
        (UiLanguage::Zh, Message::NoRepairs) => "无需执行修复编辑。",
        (UiLanguage::En, Message::NoChanges) => "No changes between source and preview.",
        (UiLanguage::Zh, Message::NoChanges) => "源码与预览之间没有差异。",
        (UiLanguage::En, Message::DiffSkipped) => "Diff skipped: file too large.",
        (UiLanguage::Zh, Message::DiffSkipped) => "文件过大，diff 已跳过。",
        (UiLanguage::En, Message::NoSchema) => "No schema loaded.",
        (UiLanguage::Zh, Message::NoSchema) => "尚未加载 Schema。",
        (UiLanguage::En, Message::SchemaValid) => "The preview matches the schema.",
        (UiLanguage::Zh, Message::SchemaValid) => "预览内容符合 Schema。",
        (UiLanguage::En, Message::OriginalProtected) => "Original file is protected",
        (UiLanguage::Zh, Message::OriginalProtected) => "原文件受到保护",
        (UiLanguage::En, Message::DropHint) => "Open or drop a JSON file to begin.",
        (UiLanguage::Zh, Message::DropHint) => "打开或拖入 JSON 文件以开始。",
        (UiLanguage::En, Message::Light) => "Light",
        (UiLanguage::Zh, Message::Light) => "浅色",
        (UiLanguage::En, Message::Dark) => "Dark",
        (UiLanguage::Zh, Message::Dark) => "深色",
        (UiLanguage::En, Message::Live) => "Live",
        (UiLanguage::Zh, Message::Live) => "实时",
        (UiLanguage::En, Message::PreviewReady) => "Preview ready",
        (UiLanguage::Zh, Message::PreviewReady) => "预览已就绪",
        (UiLanguage::En, Message::NeedsReview) => "Review repairs or schema before saving",
        (UiLanguage::Zh, Message::NeedsReview) => "保存前请先确认修复或 Schema",
        (UiLanguage::En, Message::Analyzing) => "Analyzing…",
        (UiLanguage::Zh, Message::Analyzing) => "分析中…",
        (UiLanguage::En, Message::Waiting) => "Waiting",
        (UiLanguage::Zh, Message::Waiting) => "等待输入",
    }
}

#[cfg(test)]
mod tests {
    use super::{Message, UiLanguage, text};

    #[test]
    fn english_is_the_default_language() {
        assert_eq!(UiLanguage::default(), UiLanguage::En);
        assert_eq!(text(UiLanguage::default(), Message::OpenJson), "Open JSON");
    }

    #[test]
    fn visible_labels_switch_to_chinese() {
        assert_eq!(text(UiLanguage::Zh, Message::OpenJson), "打开 JSON");
        assert_eq!(text(UiLanguage::Zh, Message::SaveFixed), "保存 .fixed");
        assert_eq!(text(UiLanguage::Zh, Message::Problems), "问题");
        assert_eq!(text(UiLanguage::Zh, Message::PreviewReady), "预览已就绪");
    }
}
