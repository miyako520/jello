use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};

pub const CJK_FONT_NAME: &str = "Noto Sans CJK SC";

pub fn font_definitions() -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    definitions.font_data.insert(
        CJK_FONT_NAME.to_string(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/NotoSansCJKsc-Regular.otf"
        ))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .get_mut(&family)
            .expect("default font family must exist")
            .push(CJK_FONT_NAME.to_string());
    }
    definitions
}
#[cfg(test)]
mod tests {
    use eframe::egui::FontFamily;
    use skrifa::{FontRef, MetadataProvider};

    use super::{CJK_FONT_NAME, font_definitions};

    #[test]
    fn bundled_fallback_font_covers_chinese_ui_and_json_text() {
        let definitions = font_definitions();
        let data = definitions
            .font_data
            .get(CJK_FONT_NAME)
            .expect("bundled CJK font must be registered");
        let font = FontRef::from_index(data.font.as_ref(), data.index)
            .expect("bundled CJK font must be valid");

        for character in "中文修复预览项目名称城市备注演示小林北京包含的".chars()
        {
            assert!(
                font.charmap().map(character).is_some(),
                "bundled font is missing {character}"
            );
        }

        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            assert_eq!(
                definitions
                    .families
                    .get(&family)
                    .and_then(|fonts| fonts.last()),
                Some(&CJK_FONT_NAME.to_string())
            );
        }
    }
}
