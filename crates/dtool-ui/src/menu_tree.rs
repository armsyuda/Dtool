use dtool_strings::text;
use serde::Serialize;
use serde_json::{json, Value};

pub mod recent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct MenuId(pub &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRowKind {
    Item,
    Toggle,
    Radio,
    Separator,
}

const ITEM: MenuRowKind = MenuRowKind::Item;
const TOG: MenuRowKind = MenuRowKind::Toggle;
const SEP: MenuRowKind = MenuRowKind::Separator;

#[derive(Debug, Clone, Serialize)]
pub struct MenuItem {
    pub id: MenuId,
    pub key: &'static str,
    pub command: Option<&'static str>,
    pub children: Vec<MenuItem>,
    pub separator: bool,
    pub planned: bool,
    pub checkable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuTree {
    pub persona: String,
    pub menus: Vec<MenuItem>,
}

pub const PERSONAS: [&str; 4] = ["pixel", "vector", "layout", "slice"];

pub const TOP_LEVEL_ORDER: [&str; 10] = [
    "file", "edit", "document", "text", "vector", "pixel", "layer", "view", "window", "help",
];

type MenuRow = (u8, &'static str, &'static str, Option<&'static str>, MenuRowKind);

const PIXEL_MENU_ROWS: &[MenuRow] = &[
    (0, "file", "menu.file", None, ITEM),
    (1, crate::home::HOME_MENU_ID, "menu.file.home", None, ITEM),
    (1, "file.studios", "menu.file.studios", None, ITEM),
    (2, "file.studios.vector", "menu.file.studios.vector", None, ITEM),
    (2, "file.studios.pixel", "menu.file.studios.pixel", None, ITEM),
    (2, "file.studios.layout", "menu.file.studios.layout", None, ITEM),
    (2, "file.studios.slice", "menu.file.studios.slice", None, ITEM),
    (2, "file.studios.retouching", "menu.file.studios.retouching", None, ITEM),
    (2, "file.studios.colour_grading", "menu.file.studios.colour_grading", None, ITEM),
    (2, "file.studios.typography", "menu.file.studios.typography", None, ITEM),
    (2, "file.studios.compositing", "menu.file.studios.compositing", None, ITEM),
    (2, "file.studios.astrophotography", "menu.file.studios.astrophotography", None, ITEM),
    (2, "file.studios.scripting", "menu.file.studios.scripting", None, ITEM),
    (1, "file.sep1", "", None, SEP),
    (1, "file.new", "menu.file.new", Some("doc.new"), ITEM),
    (1, crate::dialogs::NEW_FROM_FAVOURITES_MENU, "menu.file.new_from_favourites", None, ITEM),
    (1, crate::dialogs::NEW_FROM_LAST_PRESET_MENU, "menu.file.new_from_last_preset", None, ITEM),
    (1, "file.new_from_clipboard", "menu.file.new_from_clipboard", Some("doc.new_from_clipboard"), ITEM),
    (1, "file.new_image_process", "menu.file.new_image_process", None, ITEM),
    (2, "file.new_image_process.panorama", "menu.file.new_image_process.panorama", None, ITEM),
    (2, "file.new_image_process.stack", "menu.file.new_image_process.stack", None, ITEM),
    (2, "file.new_image_process.hdr_merge", "menu.file.new_image_process.hdr_merge", None, ITEM),
    (2, "file.new_image_process.focus_merge", "menu.file.new_image_process.focus_merge", None, ITEM),
    (2, "file.new_image_process.astrophotography_stack", "menu.file.new_image_process.astrophotography_stack", None, ITEM),
    (2, "file.new_image_process.batch_job", "menu.file.new_image_process.batch_job", None, ITEM),
    (1, "file.sep2", "", None, SEP),
    (1, "file.open", "menu.file.open", Some("doc.open"), ITEM),
    (1, "file.open_recent", "menu.file.open_recent", None, ITEM),
    (2, "file.open_recent.sep1", "", None, SEP),
    (2, "file.open_recent.clear", "menu.file.open_recent.clear", None, ITEM),
    (2, "file.open_recent.more", "menu.file.open_recent.more", None, ITEM),
    (1, "file.sep3", "", None, SEP),
    (1, "file.close", "menu.file.close", None, ITEM),
    (1, "file.close_all", "menu.file.close_all", None, ITEM),
    (1, "file.sep4", "", None, SEP),
    (1, "file.save", "menu.file.save", Some("doc.save"), ITEM),
    (1, "file.save_as", "menu.file.save_as", Some("doc.save_as"), ITEM),
    (1, "file.save_as_package", "menu.file.save_as_package", None, ITEM),
    (1, "file.sep5", "", None, SEP),
    (1, "file.save_history_with_document", "menu.file.save_history_with_document", Some("history.save_with_doc.set"), TOG),
    (1, "file.sep6", "", None, SEP),
    (1, "file.open_folder_in_explorer", "menu.file.open_folder_in_explorer", None, ITEM),
    (1, "file.sep7", "", None, SEP),
    (1, "file.place", "menu.file.place", None, ITEM),
    (1, "file.place_3d_model", "menu.file.place_3d_model", None, ITEM),
    (1, "file.placement_policy", "menu.file.placement_policy", None, ITEM),
    (2, "file.placement_policy.import_image_files_as_linked", "menu.file.placement_policy.import_image_files_as_linked", None, TOG),
    (2, "file.placement_policy.sep1", "", None, SEP),
    (2, "file.placement_policy.import_text_files_as_linked", "menu.file.placement_policy.import_text_files_as_linked", None, TOG),
    (2, "file.placement_policy.preserve_text_styles_on_import", "menu.file.placement_policy.preserve_text_styles_on_import", None, TOG),
    (1, "file.sep8", "", None, SEP),
    (1, "file.import_content", "menu.file.import_content", None, ITEM),
    (2, "file.import_content.from_file_s", "menu.file.import_from_files", None, ITEM),
    (2, "file.import_content.from_affinity_2", "menu.file.import_content.from_affinity_2", None, ITEM),
    (1, "file.import_icc_profile", "menu.file.import_icc_profile", None, ITEM),
    (1, "file.sep9", "", None, SEP),
    (1, "file.export", "menu.file.export", None, ITEM),
    (2, "file.export.export", "menu.file.export.export", Some("ui.export.open"), ITEM),
    (2, "file.export.as_template", "menu.file.export.as_template", Some("doc.template.save"), ITEM),
    (2, "file.export.lut", "menu.file.export.lut", None, ITEM),
    (1, "file.sep10", "", None, SEP),
    (1, "file.document_metadata", "menu.file.document_metadata", None, ITEM),
    (1, "file.print", "menu.file.print", None, ITEM),
    (1, "file.sep11", "", None, SEP),
    (1, crate::shell::EXIT_MENU_ID, "menu.file.exit", None, ITEM),
    (0, "edit", "menu.edit", None, ITEM),
    (1, "edit.undo", "menu.edit.undo", Some("edit.undo"), ITEM),
    (1, "edit.redo", "menu.edit.redo", Some("edit.redo"), ITEM),
    (1, "edit.sep1", "", None, SEP),
    (1, "edit.find", "menu.edit.find", None, ITEM),
    (2, "edit.find.find", "menu.edit.find.find", None, ITEM),
    (2, "edit.find.next", "menu.edit.find.next", None, ITEM),
    (2, "edit.find.previous", "menu.edit.find.previous", None, ITEM),
    (1, "edit.sep2", "", None, SEP),
    (1, "edit.select_all", "menu.edit.select_all", None, ITEM),
    (1, "edit.deselect", "menu.edit.deselect", None, ITEM),
    (1, "edit.sep3", "", None, SEP),
    (1, "edit.cut", "menu.edit.cut", Some("edit.cut"), ITEM),
    (1, "edit.copy", "menu.edit.copy", Some("edit.copy"), ITEM),
    (1, "edit.copy_flattened", "menu.edit.copy_flattened", Some("edit.copy_flattened"), ITEM),
    (1, "edit.duplicate", "menu.edit.duplicate", None, ITEM),
    (2, "edit.duplicate.layer", "menu.edit.duplicate.layer", None, ITEM),
    (2, "edit.duplicate.linked", "menu.edit.duplicate.linked", None, ITEM),
    (2, "edit.duplicate.selection", "menu.edit.duplicate.selection", Some("edit.duplicate"), ITEM),
    (1, "edit.move_duplicate", "move_duplicate.title", Some("edit.move_duplicate"), ITEM),
    (1, "edit.delete", "menu.edit.delete", None, ITEM),
    (1, "edit.paste", "menu.edit.paste", None, ITEM),
    (2, "edit.paste.paste", "menu.edit.paste.paste", Some("edit.paste"), ITEM),
    (2, "edit.paste.paste_special", "menu.edit.paste.paste_special", Some("edit.paste_special"), ITEM),
    (2, "edit.paste.style", "menu.edit.paste.style", Some("edit.paste_style"), ITEM),
    (2, "edit.paste.fx", "menu.edit.paste.fx", Some("edit.paste_fx"), ITEM),
    (2, "edit.paste.inside", "menu.edit.paste.inside", Some("edit.paste_inside"), ITEM),
    (2, "edit.paste.without_format", "menu.edit.paste.without_format", Some("edit.paste_without_format"), ITEM),
    (1, "edit.sep4", "", None, SEP),
    (1, "edit.defaults", "menu.edit.defaults", None, ITEM),
    (2, "edit.defaults.synchronise_from_selection", "menu.edit.defaults.synchronise_from_selection", None, ITEM),
    (2, "edit.defaults.save", "menu.edit.defaults.save", None, ITEM),
    (2, "edit.defaults.revert", "menu.edit.defaults.revert", None, ITEM),
    (2, "edit.defaults.factory_reset", "menu.edit.defaults.factory_reset", None, ITEM),
    (1, "edit.create_style", "menu.edit.create_style", None, ITEM),
    (1, "edit.sep5", "", None, SEP),
    (1, "edit.settings", "menu.edit.settings", None, ITEM),
    (0, "document", "menu.document", None, ITEM),
    (1, "document.pages", "menu.document.pages", None, ITEM),
    (2, "document.pages.view_masters", "menu.document.pages.view_masters", Some("master.list"), TOG),
    (2, "document.pages.sep1", "", None, SEP),
    (2, "document.pages.new", "menu.document.pages.new", Some("page.add"), ITEM),
    (2, "document.pages.sep2", "", None, SEP),
    (2, "document.pages.add", "menu.document.pages.add", Some("page.add"), ITEM),
    (2, "document.pages.delete", "menu.document.pages.delete", Some("page.delete"), ITEM),
    (2, "document.pages.sep3", "", None, SEP),
    (2, "document.pages.add_from_file", "menu.document.pages.add_from_file", Some("page.import"), ITEM),
    (2, "document.pages.sep4", "", None, SEP),
    (2, "document.pages.first", "menu.document.pages.first", Some("page.navigate"), ITEM),
    (2, "document.pages.previous", "menu.document.pages.previous", Some("page.navigate"), ITEM),
    (2, "document.pages.next", "menu.document.pages.next", Some("page.navigate"), ITEM),
    (2, "document.pages.last", "menu.document.pages.last", Some("page.navigate"), ITEM),
    (2, "document.pages.go_to", "menu.document.pages.go_to", Some("page.navigate"), ITEM),
    (2, "document.pages.previously_viewed", "menu.document.pages.previously_viewed", Some("page.navigate"), ITEM),
    (1, "document.setup", "menu.document.setup", None, ITEM),
    (2, "document.setup.document_setup", "menu.document.setup.document_setup", None, ITEM),
    (2, "document.setup.sep1", "", None, SEP),
    (2, "document.setup.convert_format_icc_profile", "menu.document.setup.convert_format_icc_profile", None, ITEM),
    (2, "document.setup.assign_icc_profile", "menu.doc.assign_icc", None, ITEM),
    (2, "document.setup.sep2", "", None, SEP),
    (2, "document.setup.resize", "menu.document.setup.resize", None, ITEM),
    (3, "document.setup.resize.document", "menu.document.setup.resize.document", None, ITEM),
    (3, "document.setup.resize.canvas", "menu.document.setup.resize.canvas", None, ITEM),
    (3, "document.setup.resize.pixel_art_document", "menu.document.setup.resize.pixel_art_document", None, ITEM),
    (2, "document.setup.sep3", "", None, SEP),
    (2, "document.setup.transparent_background", "menu.document.setup.transparent_background", None, ITEM),
    (1, "document.sep1", "", None, SEP),
    (1, "document.clip_canvas", "menu.document.clip_canvas", None, ITEM),
    (1, "document.unclip_canvas", "menu.document.unclip_canvas", None, ITEM),
    (1, "document.sep2", "", None, SEP),
    (1, "document.rotate_90_clockwise", "menu.document.rotate_90_clockwise", None, ITEM),
    (1, "document.rotate_90_anticlockwise", "menu.document.rotate_90_anticlockwise", None, ITEM),
    (1, "document.flip_horizontal", "menu.document.flip_horizontal", None, ITEM),
    (1, "document.flip_vertical", "menu.document.flip_vertical", None, ITEM),
    (1, "document.sep3", "", None, SEP),
    (1, "document.add_snapshot", "menu.document.add_snapshot", None, ITEM),
    (1, "document.restore_snapshot", "menu.document.restore_snapshot", None, ITEM),
    (2, "document.restore_snapshot.none", "menu.document.restore_snapshot.none", None, ITEM),
    (1, "document.sep4", "", None, SEP),
    (1, "document.resource_manager", "menu.document.resource_manager", None, ITEM),
    (1, "document.section_manager", "menu.document.section_manager", None, ITEM),
    (1, "document.font_manager", "menu.document.font_manager", None, ITEM),
    (1, "document.sep5", "", None, SEP),
    (1, "document.flatten", "menu.document.flatten", None, ITEM),
    (0, "text", "menu.text", None, ITEM),
    (1, "text.character_traits", "menu.text.character_traits", None, ITEM),
    (2, "text.character_traits.bold", "menu.text.character_traits.bold", None, TOG),
    (2, "text.character_traits.italic", "menu.text.character_traits.italic", None, TOG),
    (2, "text.character_traits.underline", "menu.text.character_traits.underline", None, TOG),
    (2, "text.character_traits.strikethrough", "menu.text.character_traits.strikethrough", None, TOG),
    (1, "text.size", "menu.text.size", None, ITEM),
    (2, "text.size.bigger", "menu.text.size.bigger", None, TOG),
    (2, "text.size.smaller", "menu.text.size.smaller", None, TOG),
    (2, "text.size.precise_bigger", "menu.text.size.precise_bigger", None, TOG),
    (2, "text.size.precise_smaller", "menu.text.size.precise_smaller", None, TOG),
    (1, "text.spacing", "menu.text.spacing", None, ITEM),
    (2, "text.spacing.use_default", "menu.text.spacing.use_default", None, TOG),
    (2, "text.spacing.use_none", "menu.text.spacing.use_none", None, TOG),
    (2, "text.spacing.sep1", "", None, SEP),
    (2, "text.spacing.tighten", "menu.text.spacing.tighten", None, TOG),
    (2, "text.spacing.loosen", "menu.text.spacing.loosen", None, TOG),
    (2, "text.spacing.tighten_more", "menu.text.spacing.tighten_more", None, TOG),
    (2, "text.spacing.loosen_more", "menu.text.spacing.loosen_more", None, TOG),
    (1, "text.ligatures", "menu.text.ligatures", None, ITEM),
    (2, "text.ligatures.use_default", "menu.text.ligatures.use_default", None, TOG),
    (2, "text.ligatures.use_none", "menu.text.ligatures.use_none", None, TOG),
    (2, "text.ligatures.use_all", "menu.text.ligatures.use_all", None, TOG),
    (1, "text.baseline", "menu.text.baseline", None, ITEM),
    (2, "text.baseline.use_default", "menu.text.baseline.use_default", None, TOG),
    (2, "text.baseline.superscript", "menu.text.baseline.superscript", None, TOG),
    (2, "text.baseline.subscript", "menu.text.baseline.subscript", None, TOG),
    (2, "text.baseline.sep1", "", None, SEP),
    (2, "text.baseline.raise", "menu.text.baseline.raise", None, TOG),
    (2, "text.baseline.lower", "menu.text.baseline.lower", None, TOG),
    (2, "text.baseline.precise_raise", "menu.text.baseline.precise_raise", None, TOG),
    (2, "text.baseline.precise_lower", "menu.text.baseline.precise_lower", None, TOG),
    (1, "text.capitalisation", "menu.text.capitalisation", None, ITEM),
    (2, "text.capitalisation.none", "menu.text.capitalisation.none", None, TOG),
    (2, "text.capitalisation.small_caps", "menu.text.capitalisation.small_caps", None, TOG),
    (2, "text.capitalisation.all_caps", "menu.text.capitalisation.all_caps", None, TOG),
    (2, "text.capitalisation.sep1", "", None, SEP),
    (2, "text.capitalisation.lower_case", "menu.text.capitalisation.lower_case", None, TOG),
    (2, "text.capitalisation.upper_case", "menu.text.capitalisation.upper_case", None, TOG),
    (2, "text.capitalisation.toggle_case", "menu.text.capitalisation.toggle_case", None, TOG),
    (2, "text.capitalisation.title_case", "menu.text.capitalisation.title_case", None, TOG),
    (2, "text.capitalisation.sentence_case", "menu.text.capitalisation.sentence_case", None, TOG),
    (1, "text.list", "menu.text.list", None, ITEM),
    (2, "text.list.bullet", "menu.text.list.bullet", None, TOG),
    (2, "text.list.number", "menu.text.list.number", None, TOG),
    (2, "text.list.restart_numbering", "menu.text.list.restart_numbering", None, TOG),
    (2, "text.list.increase_level", "menu.text.list.increase_level", None, TOG),
    (2, "text.list.decrease_level", "menu.text.list.decrease_level", None, TOG),
    (1, "text.table", "menu.text.table", None, ITEM),
    (2, "text.table.insert_column", "menu.text.table.insert_column", Some("table.column.insert"), ITEM),
    (2, "text.table.delete_column_s", "menu.text.table.delete_column_s", Some("table.column.delete"), ITEM),
    (2, "text.table.insert_row", "menu.text.table.insert_row", Some("table.row.insert"), ITEM),
    (2, "text.table.delete_row_s", "menu.text.table.delete_row_s", Some("table.row.delete"), ITEM),
    (2, "text.table.sep1", "", None, SEP),
    (2, "text.table.merge_cells", "menu.text.table.merge_cells", Some(dtool_core::table::merge::MERGE), ITEM),
    (2, "text.table.separate_cells", "menu.text.table.separate_cells", Some(dtool_core::table::merge::UNMERGE), ITEM),
    (2, "text.table.sep2", "", None, SEP),
    (2, "text.table.evenly_distribute_columns", "menu.text.table.evenly_distribute_columns", Some("table.columns.distribute"), ITEM),
    (2, "text.table.evenly_distribute_rows", "menu.text.table.evenly_distribute_rows", Some("table.rows.distribute"), ITEM),
    (2, "text.table.sep3", "", None, SEP),
    (2, "text.table.autofit_rows", "menu.text.table.autofit_rows", Some("table.row.autofit"), ITEM),
    (2, "text.table.autofit_columns", "menu.text.table.autofit_columns", Some("table.column.autofit"), ITEM),
    (2, "text.table.sep4", "", None, SEP),
    (2, "text.table.sort", "menu.text.table.sort", None, ITEM),
    (3, "text.table.sort.sort_by_row_ascending", "menu.text.table.sort.sort_by_row_ascending", Some(dtool_core::table::sort::COLUMNS), ITEM),
    (3, "text.table.sort.sort_by_row_descending", "menu.text.table.sort.sort_by_row_descending", Some(dtool_core::table::sort::COLUMNS), ITEM),
    (3, "text.table.sort.sort_by_column_ascending", "menu.text.table.sort.sort_by_column_ascending", Some(dtool_core::table::sort::ROWS), ITEM),
    (3, "text.table.sort.sort_by_column_descending", "menu.text.table.sort.sort_by_column_descending", Some(dtool_core::table::sort::ROWS), ITEM),
    (3, "text.table.sort.sort_options", "menu.text.table.sort.sort_options", Some(dtool_core::table::sort::OPTIONS_QUERY), ITEM),
    (2, "text.table.sep5", "", None, SEP),
    (2, "text.table.save_formats_as_default", "menu.text.table.save_formats_as_default", Some(dtool_core::table::styles::DEFAULT_SAVE), ITEM),
    (1, "text.sep1", "", None, SEP),
    (1, "text.alignment", "menu.text.alignment", None, ITEM),
    (2, "text.alignment.align_left", "menu.text.alignment.align_left", None, TOG),
    (2, "text.alignment.align_centre", "menu.text.alignment.align_centre", None, TOG),
    (2, "text.alignment.align_right", "menu.text.alignment.align_right", None, TOG),
    (2, "text.alignment.sep1", "", None, SEP),
    (2, "text.alignment.justify_left", "menu.text.alignment.justify_left", None, TOG),
    (2, "text.alignment.justify_centre", "menu.text.alignment.justify_centre", None, TOG),
    (2, "text.alignment.justify_right", "menu.text.alignment.justify_right", None, TOG),
    (2, "text.alignment.justify_all", "menu.text.alignment.justify_all", None, TOG),
    (2, "text.alignment.sep2", "", None, SEP),
    (2, "text.alignment.align_towards_spine", "menu.text.alignment.align_towards_spine", None, TOG),
    (2, "text.alignment.align_away_from_spine", "menu.text.alignment.align_away_from_spine", None, TOG),
    (1, "text.paragraph_leading", "menu.text.paragraph_leading", None, ITEM),
    (2, "text.paragraph_leading.increase", "menu.text.paragraph_leading.increase", None, TOG),
    (2, "text.paragraph_leading.decrease", "menu.text.paragraph_leading.decrease", None, TOG),
    (2, "text.paragraph_leading.precise_increase", "menu.text.paragraph_leading.precise_increase", None, TOG),
    (2, "text.paragraph_leading.precise_decrease", "menu.text.paragraph_leading.precise_decrease", None, TOG),
    (1, "text.vertical_alignment", "menu.text.vertical_alignment", None, ITEM),
    (2, "text.vertical_alignment.top", "menu.text.vertical_alignment.top", None, TOG),
    (2, "text.vertical_alignment.centre", "menu.text.vertical_alignment.centre", None, TOG),
    (2, "text.vertical_alignment.bottom", "menu.text.vertical_alignment.bottom", None, TOG),
    (2, "text.vertical_alignment.justify", "menu.text.vertical_alignment.justify", None, TOG),
    (1, "text.sep2", "", None, SEP),
    (1, "text.text_styles", "menu.text.text_styles", None, ITEM),
    (2, "text.text_styles.reapply_text_styles", "menu.text.text_styles.reapply_text_styles", None, ITEM),
    (2, "text.text_styles.reapply_base_styles", "menu.text.text_styles.reapply_base_styles", None, ITEM),
    (2, "text.text_styles.sep1", "", None, SEP),
    (2, "text.text_styles.create_paragraph_style", "menu.text.text_styles.create_paragraph_style", None, ITEM),
    (2, "text.text_styles.update_paragraph_style", "menu.text.text_styles.update_paragraph_style", None, ITEM),
    (2, "text.text_styles.delete_paragraph_style", "menu.text.text_styles.delete_paragraph_style", None, ITEM),
    (2, "text.text_styles.set_no_paragraph_style", "menu.text.text_styles.set_no_paragraph_style", None, ITEM),
    (2, "text.text_styles.sep2", "", None, SEP),
    (2, "text.text_styles.create_character_style", "menu.text.text_styles.create_character_style", None, ITEM),
    (2, "text.text_styles.update_character_style", "menu.text.text_styles.update_character_style", None, ITEM),
    (2, "text.text_styles.delete_character_style", "menu.text.text_styles.delete_character_style", None, ITEM),
    (2, "text.text_styles.set_no_character_style", "menu.text.text_styles.set_no_character_style", None, ITEM),
    (2, "text.text_styles.sep3", "", None, SEP),
    (2, "text.text_styles.delete_unused_styles", "menu.text.text_styles.delete_unused_styles", None, ITEM),
    (2, "text.text_styles.detach_and_delete_all_styles", "menu.text.text_styles.detach_and_delete_all_styles", None, ITEM),
    (2, "text.text_styles.save_text_styles_as_default", "menu.text.text_styles.save_text_styles_as_default", None, ITEM),
    (2, "text.text_styles.import_text_styles", "menu.text.text_styles.import_text_styles", None, ITEM),
    (1, "text.sep3", "", None, SEP),
    (1, "text.insert", "menu.text.insert", None, ITEM),
    (2, "text.insert.insert_filler_text", "menu.text.filler", None, ITEM),
    (2, "text.insert.fields", "menu.text.insert.fields", None, ITEM),
    (3, "text.insert.fields.page_number", "menu.text.insert.fields.page_number", Some("text.insert.fields.page_number"), ITEM),
    (3, "text.insert.fields.next_frame_page_number", "menu.text.insert.fields.next_frame_page_number", Some("text.insert.fields.next_frame_page_number"), ITEM),
    (3, "text.insert.fields.previous_frame_page_number", "menu.text.insert.fields.previous_frame_page_number", Some("text.insert.fields.previous_frame_page_number"), ITEM),
    (3, "text.insert.fields.total_pages", "menu.text.insert.fields.total_pages", Some("text.insert.fields.total_pages"), ITEM),
    (3, "text.insert.fields.last_page_in_section_run", "menu.text.insert.fields.last_page_in_section_run", Some("text.insert.fields.last_page_in_section_run"), ITEM),
    (3, "text.insert.fields.section_name", "menu.text.insert.fields.section_name", Some("text.insert.fields.section_name"), ITEM),
    (3, "text.insert.fields.artboard_name", "menu.text.insert.fields.artboard_name", Some("text.insert.fields.artboard_name"), ITEM),
    (3, "text.insert.fields.running_header", "menu.text.insert.fields.running_header", Some("text.insert.fields.running_header"), ITEM),
    (3, "text.insert.fields.sep1", "", None, SEP),
    (3, "text.insert.fields.date_and_time", "menu.text.insert.fields.date_and_time", Some("text.insert.fields.date_and_time"), ITEM),
    (3, "text.insert.fields.created_date", "menu.text.insert.fields.created_date", Some("text.insert.fields.created_date"), ITEM),
    (3, "text.insert.fields.saved_date", "menu.text.insert.fields.saved_date", Some("text.insert.fields.saved_date"), ITEM),
    (3, "text.insert.fields.printed_or_exported_date", "menu.text.insert.fields.printed_or_exported_date", Some("text.insert.fields.printed_or_exported_date"), ITEM),
    (3, "text.insert.fields.sep2", "", None, SEP),
    (3, "text.insert.fields.more", "menu.text.insert.fields.more", None, ITEM),
    (2, "text.insert.symbols", "menu.text.insert.symbols", None, ITEM),
    (3, "text.insert.symbols.bullet", "menu.text.insert.symbols.bullet", None, TOG),
    (3, "text.insert.symbols.dagger", "menu.text.insert.symbols.dagger", None, TOG),
    (3, "text.insert.symbols.double_dagger", "menu.text.insert.symbols.double_dagger", None, TOG),
    (3, "text.insert.symbols.ellipsis", "menu.text.insert.symbols.ellipsis", None, TOG),
    (3, "text.insert.symbols.sep1", "", None, SEP),
    (3, "text.insert.symbols.inverted_question_mark", "menu.text.insert.symbols.inverted_question_mark", None, TOG),
    (3, "text.insert.symbols.inverted_exclamation_mark", "menu.text.insert.symbols.inverted_exclamation_mark", None, TOG),
    (3, "text.insert.symbols.sep2", "", None, SEP),
    (3, "text.insert.symbols.copyright", "menu.text.insert.symbols.copyright", None, TOG),
    (3, "text.insert.symbols.trademark", "menu.text.insert.symbols.trademark", None, TOG),
    (3, "text.insert.symbols.registered_trademark", "menu.text.insert.symbols.registered_trademark", None, TOG),
    (3, "text.insert.symbols.sep3", "", None, SEP),
    (3, "text.insert.symbols.paragraph_marker", "menu.text.insert.symbols.paragraph_marker", None, TOG),
    (3, "text.insert.symbols.section_marker", "menu.text.insert.symbols.section_marker", None, TOG),
    (2, "text.insert.maths", "menu.text.insert.maths", None, ITEM),
    (3, "text.insert.maths.plus_minus", "menu.text.insert.maths.plus_minus", None, TOG),
    (3, "text.insert.maths.multiply", "menu.text.insert.maths.multiply", None, TOG),
    (3, "text.insert.maths.divide", "menu.text.insert.maths.divide", None, TOG),
    (3, "text.insert.maths.sep1", "", None, SEP),
    (3, "text.insert.maths.approximately_equal", "menu.text.insert.maths.approximately_equal", None, TOG),
    (3, "text.insert.maths.not_equal", "menu.text.insert.maths.not_equal", None, TOG),
    (3, "text.insert.maths.less_than_or_equal", "menu.text.insert.maths.less_than_or_equal", None, TOG),
    (3, "text.insert.maths.greater_than_or_equal", "menu.text.insert.maths.greater_than_or_equal", None, TOG),
    (3, "text.insert.maths.sep2", "", None, SEP),
    (3, "text.insert.maths.degree", "menu.text.insert.maths.degree", None, TOG),
    (3, "text.insert.maths.per_mille", "menu.text.insert.maths.per_mille", None, TOG),
    (3, "text.insert.maths.pi", "menu.text.insert.maths.pi", None, TOG),
    (3, "text.insert.maths.infinity", "menu.text.insert.maths.infinity", None, TOG),
    (2, "text.insert.quotation_marks", "menu.text.insert.quotation_marks", None, ITEM),
    (3, "text.insert.quotation_marks.double_left_quote", "menu.text.insert.quotation_marks.double_left_quote", None, TOG),
    (3, "text.insert.quotation_marks.double_right_quote", "menu.text.insert.quotation_marks.double_right_quote", None, TOG),
    (3, "text.insert.quotation_marks.single_left_quote", "menu.text.insert.quotation_marks.single_left_quote", None, TOG),
    (3, "text.insert.quotation_marks.single_right_quote", "menu.text.insert.quotation_marks.single_right_quote", None, TOG),
    (3, "text.insert.quotation_marks.sep1", "", None, SEP),
    (3, "text.insert.quotation_marks.double_base_quote", "menu.text.insert.quotation_marks.double_base_quote", None, TOG),
    (3, "text.insert.quotation_marks.single_base_quote", "menu.text.insert.quotation_marks.single_base_quote", None, TOG),
    (3, "text.insert.quotation_marks.sep2", "", None, SEP),
    (3, "text.insert.quotation_marks.left_guillemet", "menu.text.insert.quotation_marks.left_guillemet", None, TOG),
    (3, "text.insert.quotation_marks.right_guillemet", "menu.text.insert.quotation_marks.right_guillemet", None, TOG),
    (3, "text.insert.quotation_marks.single_left_guillemet", "menu.text.insert.quotation_marks.single_left_guillemet", None, TOG),
    (3, "text.insert.quotation_marks.single_right_guillemet", "menu.text.insert.quotation_marks.single_right_guillemet", None, TOG),
    (2, "text.insert.dashes_and_hyphens", "menu.text.insert.dashes_and_hyphens", None, ITEM),
    (3, "text.insert.dashes_and_hyphens.em_dash", "menu.text.insert.dashes_and_hyphens.em_dash", None, TOG),
    (3, "text.insert.dashes_and_hyphens.en_dash", "menu.text.insert.dashes_and_hyphens.en_dash", None, TOG),
    (3, "text.insert.dashes_and_hyphens.figure_dash", "menu.text.insert.dashes_and_hyphens.figure_dash", None, TOG),
    (3, "text.insert.dashes_and_hyphens.sep1", "", None, SEP),
    (3, "text.insert.dashes_and_hyphens.soft_hyphen", "menu.text.insert.dashes_and_hyphens.soft_hyphen", None, TOG),
    (3, "text.insert.dashes_and_hyphens.non_breaking_hyphen", "menu.text.insert.dashes_and_hyphens.non_breaking_hyphen", None, TOG),
    (2, "text.insert.spaces_and_tabs", "menu.text.insert.spaces_and_tabs", None, ITEM),
    (3, "text.insert.spaces_and_tabs.em_space", "menu.text.insert.spaces_and_tabs.em_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.en_space", "menu.text.insert.spaces_and_tabs.en_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.sep1", "", None, SEP),
    (3, "text.insert.spaces_and_tabs.non_breaking_space", "menu.text.insert.spaces_and_tabs.non_breaking_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.narrow_non_breaking_space", "menu.text.insert.spaces_and_tabs.narrow_non_breaking_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.sep2", "", None, SEP),
    (3, "text.insert.spaces_and_tabs.zero_width_space", "menu.text.insert.spaces_and_tabs.zero_width_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.hair_space", "menu.text.insert.spaces_and_tabs.hair_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.sixth_space", "menu.text.insert.spaces_and_tabs.sixth_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.thin_space", "menu.text.insert.spaces_and_tabs.thin_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.quarter_space", "menu.text.insert.spaces_and_tabs.quarter_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.third_space", "menu.text.insert.spaces_and_tabs.third_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.punctuation_space", "menu.text.insert.spaces_and_tabs.punctuation_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.figure_space", "menu.text.insert.spaces_and_tabs.figure_space", None, TOG),
    (3, "text.insert.spaces_and_tabs.sep3", "", None, SEP),
    (3, "text.insert.spaces_and_tabs.zero_width_non_joiner", "menu.text.insert.spaces_and_tabs.zero_width_non_joiner", None, TOG),
    (3, "text.insert.spaces_and_tabs.sep4", "", None, SEP),
    (3, "text.insert.spaces_and_tabs.tab", "menu.text.insert.spaces_and_tabs.tab", None, TOG),
    (3, "text.insert.spaces_and_tabs.right_indent_tab", "menu.text.insert.spaces_and_tabs.right_indent_tab", None, TOG),
    (3, "text.insert.spaces_and_tabs.indent_to_here", "menu.text.insert.spaces_and_tabs.indent_to_here", None, TOG),
    (2, "text.insert.breaks", "menu.text.insert.breaks", None, ITEM),
    (3, "text.insert.breaks.line_break", "menu.text.insert.breaks.line_break", None, TOG),
    (3, "text.insert.breaks.column_break", "menu.text.insert.breaks.column_break", None, TOG),
    (3, "text.insert.breaks.frame_break", "menu.text.insert.breaks.frame_break", None, TOG),
    (3, "text.insert.breaks.page_break", "menu.text.insert.breaks.page_break", None, TOG),
    (3, "text.insert.breaks.odd_page_break", "menu.text.insert.breaks.odd_page_break", None, TOG),
    (3, "text.insert.breaks.even_page_break", "menu.text.insert.breaks.even_page_break", None, TOG),
    (1, "text.expand", "menu.text.expand", None, ITEM),
    (2, "text.expand.field", "menu.text.expand.field", None, ITEM),
    (2, "text.expand.list", "menu.text.expand.list", None, ITEM),
    (1, "text.sep4", "", None, SEP),
    (1, "text.toggle_unicode", "menu.text.toggle_unicode", Some("text.toggle_unicode"), ITEM),
    (1, "text.interactive", "menu.text.interactive", None, ITEM),
    (2, "text.interactive.insert_anchor", "menu.text.interactive.insert_anchor", None, ITEM),
    (2, "text.interactive.show_anchors", "menu.text.interactive.show_anchors", None, TOG),
    (2, "text.interactive.sep1", "", None, SEP),
    (2, "text.interactive.insert_hyperlink", "menu.text.interactive.insert_hyperlink", None, ITEM),
    (2, "text.interactive.show_hyperlinks", "menu.text.interactive.show_hyperlinks", None, TOG),
    (1, "text.spelling", "menu.text.spelling", None, ITEM),
    (2, "text.spelling.show_spelling_options", "menu.text.spelling.show_spelling_options", None, ITEM),
    (2, "text.spelling.check_spelling", "menu.text.spelling.check_spelling", None, TOG),
    (2, "text.spelling.sep1", "", None, SEP),
    (2, "text.spelling.check_spelling_while_typing", "menu.text.spelling.check_spelling_while_typing", None, TOG),
    (2, "text.spelling.sep2", "", None, SEP),
    (2, "text.spelling.ignore_misspelled_word", "menu.text.spelling.ignore_misspelled_word", None, TOG),
    (1, "text.sep5", "", None, SEP),
    (1, "text.notes", "menu.text.notes", None, ITEM),
    (2, "text.notes.insert_footnote", "menu.text.notes.insert_footnote", None, TOG),
    (2, "text.notes.insert_sidenote", "menu.text.notes.insert_sidenote", None, TOG),
    (2, "text.notes.insert_endnote", "menu.text.notes.insert_endnote", None, TOG),
    (2, "text.notes.sep1", "", None, SEP),
    (2, "text.notes.convert_notes", "menu.text.notes.convert_notes", None, ITEM),
    (2, "text.notes.sep2", "", None, SEP),
    (2, "text.notes.go_to_note_body", "menu.text.notes.go_to_note_body", None, TOG),
    (2, "text.notes.go_to_note_reference", "menu.text.notes.go_to_note_reference", None, TOG),
    (2, "text.notes.sep3", "", None, SEP),
    (2, "text.notes.show_note_marks", "menu.text.notes.show_note_marks", None, TOG),
    (1, "text.cross_references", "menu.text.cross_references", None, ITEM),
    (2, "text.cross_references.update", "menu.text.cross_references.update", None, TOG),
    (2, "text.cross_references.update_all", "menu.text.cross_references.update_all", None, TOG),
    (1, "text.table_of_contents", "menu.text.table_of_contents", None, ITEM),
    (2, "text.table_of_contents.insert", "menu.text.table_of_contents.insert", None, TOG),
    (2, "text.table_of_contents.update", "menu.text.table_of_contents.update", None, ITEM),
    (2, "text.table_of_contents.update_all", "menu.text.table_of_contents.update_all", None, ITEM),
    (1, "text.index", "menu.text.index", None, ITEM),
    (2, "text.index.insert_index_mark", "menu.text.index.insert_index_mark", None, TOG),
    (2, "text.index.sep1", "", None, SEP),
    (2, "text.index.insert_index", "menu.text.index.insert_index", None, TOG),
    (2, "text.index.update_index", "menu.text.index.update_index", None, ITEM),
    (2, "text.index.sep2", "", None, SEP),
    (2, "text.index.show_index_marks", "menu.text.index.show_index_marks", None, TOG),
    (1, "text.sep6", "", None, SEP),
    (1, "text.text_wrap", "menu.text.text_wrap", None, ITEM),
    (2, "text.text_wrap.show_text_wrap_settings", "menu.text.text_wrap.show_text_wrap_settings", None, ITEM),
    (2, "text.text_wrap.edit_wrap_outline", "menu.text.text_wrap.edit_wrap_outline", None, ITEM),
    (2, "text.text_wrap.reset_wrap_outline", "menu.text.text_wrap.reset_wrap_outline", None, ITEM),
    (2, "text.text_wrap.sep1", "", None, SEP),
    (2, "text.text_wrap.ignore_text_wraps", "menu.text.text_wrap.ignore_text_wraps", None, TOG),
    (1, "text.sep7", "", None, SEP),
    (1, "text.show_special_characters", "menu.text.show_special_characters", None, TOG),
    (1, "text.highlights", "menu.text.highlights", None, ITEM),
    (2, "text.highlights.highlight_fields", "menu.text.highlights.highlight_fields", None, TOG),
    (2, "text.highlights.highlight_missing_fonts", "menu.text.highlights.highlight_missing_fonts", Some("view.flags.set"), TOG),
    (2, "text.highlights.highlight_missing_characters", "menu.text.highlights.highlight_missing_characters", Some("view.flags.set"), TOG),
    (0, "vector", "menu.vector", None, ITEM),
    (1, "vector.new_container_layer", "menu.vector.new_container_layer", None, ITEM),
    (1, "vector.create_symbol", "menu.vector.create_symbol", Some("symbol.create"), ITEM),
    (1, "vector.promote_group_to_layer", "menu.vector.promote_group_to_layer", Some("node.folder.promote"), ITEM),
    (1, "vector.convert_to_curves", "menu.vector.convert_to_curves", None, ITEM),
    (1, "vector.sep1", "", None, SEP),
    (1, "vector.convert_object_to_artboard", "menu.vector.convert_object_to_artboard", Some("vector.convert_object_to_artboard"), ITEM),
    (1, "vector.convert_artboard_to_object", "menu.vector.convert_artboard_to_object", Some("vector.convert_artboard_to_object"), ITEM),
    (1, "vector.sep2", "", None, SEP),
    (1, "vector.geometry", "menu.vector.geometry", None, ITEM),
    (2, "vector.geometry.add", "menu.vector.geometry.add", Some("boolean.add"), ITEM),
    (2, "vector.geometry.subtract", "menu.vector.geometry.subtract", Some("boolean.subtract"), ITEM),
    (2, "vector.geometry.intersect", "menu.vector.geometry.intersect", Some("boolean.intersect"), ITEM),
    (2, "vector.geometry.xor", "menu.vector.geometry.xor", Some("boolean.xor"), ITEM),
    (2, "vector.geometry.divide", "menu.vector.geometry.divide", Some("boolean.divide"), ITEM),
    (2, "vector.geometry.destructive", "menu.vector.geometry.destructive", None, ITEM),
    (3, "vector.geometry.destructive.destructive_add", "menu.vector.geometry.destructive.destructive_add", Some("boolean.add"), ITEM),
    (3, "vector.geometry.destructive.destructive_subtract", "menu.vector.geometry.destructive.destructive_subtract", Some("boolean.subtract"), ITEM),
    (3, "vector.geometry.destructive.destructive_intersect", "menu.vector.geometry.destructive.destructive_intersect", Some("boolean.intersect"), ITEM),
    (3, "vector.geometry.destructive.destructive_xor", "menu.vector.geometry.destructive.destructive_xor", Some("boolean.xor"), ITEM),
    (2, "vector.geometry.curve_cleanup", "", None, SEP),
    (2, "vector.geometry.fill_holes", "menu.vector.fill_holes", Some("path.curves.fill_holes"), ITEM),
    (2, "vector.geometry.remove_inner_curves", "menu.vector.remove_inner_curves", Some("path.curves.remove_inner"), ITEM),
    (2, "vector.geometry.cut_curves_with_key_object", "menu.vector.cut_curves_with_key_object", Some("path.curves.cut_with_key"), ITEM),
    (1, "vector.new_warp_group", "menu.vector.new_warp_group", None, ITEM),
    (2, "vector.new_warp_group.mesh", "menu.vector.new_warp_group.mesh", None, ITEM),
    (2, "vector.new_warp_group.quad", "menu.vector.new_warp_group.quad", None, ITEM),
    (2, "vector.new_warp_group.perspective", "menu.vector.new_warp_group.perspective", None, ITEM),
    (2, "vector.new_warp_group.arc_vertical", "menu.vector.new_warp_group.arc_vertical", None, ITEM),
    (2, "vector.new_warp_group.arc_horizontal", "menu.vector.new_warp_group.arc_horizontal", None, ITEM),
    (2, "vector.new_warp_group.bend_vertical", "menu.vector.new_warp_group.bend_vertical", None, ITEM),
    (2, "vector.new_warp_group.bend_horizontal", "menu.vector.new_warp_group.bend_horizontal", None, ITEM),
    (2, "vector.new_warp_group.fish_eye", "menu.vector.new_warp_group.fish_eye", None, ITEM),
    (2, "vector.new_warp_group.twist", "menu.vector.new_warp_group.twist", None, ITEM),
    (2, "vector.new_warp_group.arch", "menu.vector.new_warp_group.arch", None, ITEM),
    (2, "vector.new_warp_group.upper_arc", "menu.vector.new_warp_group.upper_arc", None, ITEM),
    (2, "vector.new_warp_group.lower_arc", "menu.vector.new_warp_group.lower_arc", None, ITEM),
    (2, "vector.new_warp_group.bulge", "menu.vector.new_warp_group.bulge", None, ITEM),
    (2, "vector.new_warp_group.flag", "menu.vector.new_warp_group.flag", None, ITEM),
    (2, "vector.new_warp_group.rise", "menu.vector.new_warp_group.rise", None, ITEM),
    (1, "vector.sep3", "", None, SEP),
    (1, "vector.create_compound", "menu.vector.create_compound", Some("boolean.add"), ITEM),
    (1, "vector.edit_compound", "menu.vector.edit_compound", Some("compound.edit"), ITEM),
    (1, "vector.release_compound", "menu.vector.release_compound", Some("compound.release"), ITEM),
    (1, "vector.sep4", "", None, SEP),
    (1, "vector.join_curves", "menu.vector.join_curves", Some("path.join"), ITEM),
    (1, "vector.merge_curves", "menu.vector.merge_curves", Some("path.curves.merge"), ITEM),
    (1, "vector.separate_curves", "menu.vector.separate_curves", Some("path.curves.separate"), ITEM),
    (1, "vector.sep5", "", None, SEP),
    (1, "vector.image_trace", "menu.vector.image_trace", Some("image.trace"), ITEM),
    (1, "vector.sep6", "", None, SEP),
    (1, "vector.expand_stroke", "menu.vector.expand_stroke", Some("stroke.expand"), ITEM),
    (1, "vector.fill_holes", "menu.vector.fill_holes", Some("path.curves.fill_holes"), ITEM),
    (1, "vector.remove_inner_curves", "menu.vector.remove_inner_curves", Some("path.curves.remove_inner"), ITEM),
    (1, "vector.fill_mode", "menu.vector.fill_mode", None, ITEM),
    (2, "vector.fill_mode.alternate_even_odd", "menu.vector.fill_mode.alternate_even_odd", Some("path.fill_rule.set"), MenuRowKind::Radio),
    (2, "vector.fill_mode.winding_non_zero", "menu.vector.fill_mode.winding_non_zero", Some("path.fill_rule.set"), MenuRowKind::Radio),
    (1, "vector.sep7", "", None, SEP),
    (1, "vector.cut_curves_with_key_object", "menu.vector.cut_curves_with_key_object", Some("path.curves.cut_with_key"), ITEM),
    (0, "pixel", "menu.pixel", None, ITEM),
    (1, "pixel.filters", "menu.pixel.filters", None, ITEM),
    (2, "pixel.filters.fade", "menu.pixel.filters.fade", None, ITEM),
    (2, "pixel.filters.sep1", "", None, SEP),
    (2, "pixel.filters.blur", "menu.pixel.filters.blur", None, ITEM),
    (3, "pixel.filters.blur.average", "menu.pixel.filters.blur.average", None, ITEM),
    (3, "pixel.filters.blur.gaussian", "menu.pixel.filters.blur.gaussian", None, ITEM),
    (3, "pixel.filters.blur.diffuse_glow", "menu.pixel.filters.blur.diffuse_glow", None, ITEM),
    (3, "pixel.filters.blur.bloom", "menu.pixel.filters.blur.bloom", None, ITEM),
    (3, "pixel.filters.blur.box", "menu.pixel.filters.blur.box", None, ITEM),
    (3, "pixel.filters.blur.motion", "menu.pixel.filters.blur.motion", None, ITEM),
    (3, "pixel.filters.blur.radial", "menu.pixel.filters.blur.radial", None, ITEM),
    (3, "pixel.filters.blur.zoom", "menu.pixel.filters.blur.zoom", None, ITEM),
    (3, "pixel.filters.blur.lens", "menu.pixel.filters.blur.lens", None, ITEM),
    (3, "pixel.filters.blur.depth_of_field", "menu.pixel.filters.blur.depth_of_field", None, ITEM),
    (3, "pixel.filters.blur.field", "menu.pixel.filters.blur.field", None, ITEM),
    (3, "pixel.filters.blur.sep1", "", None, SEP),
    (3, "pixel.filters.blur.maximum", "menu.pixel.filters.blur.maximum", None, ITEM),
    (3, "pixel.filters.blur.minimum", "menu.pixel.filters.blur.minimum", None, ITEM),
    (3, "pixel.filters.blur.sep2", "", None, SEP),
    (3, "pixel.filters.blur.bilateral", "menu.pixel.filters.blur.bilateral", None, ITEM),
    (3, "pixel.filters.blur.median", "menu.pixel.filters.blur.median", None, ITEM),
    (3, "pixel.filters.blur.custom", "menu.pixel.filters.blur.custom", None, ITEM),
    (2, "pixel.filters.sharpen", "menu.pixel.filters.sharpen", None, ITEM),
    (3, "pixel.filters.sharpen.unsharp_mask", "menu.pixel.filters.sharpen.unsharp_mask", None, ITEM),
    (3, "pixel.filters.sharpen.clarity", "menu.pixel.filters.sharpen.clarity", None, ITEM),
    (3, "pixel.filters.sharpen.texture", "menu.pixel.filters.sharpen.texture", None, ITEM),
    (3, "pixel.filters.sharpen.high_pass", "menu.pixel.filters.sharpen.high_pass", None, ITEM),
    (3, "pixel.filters.sharpen.multi_band", "menu.pixel.filters.sharpen.multi_band", None, ITEM),
    (2, "pixel.filters.distort", "menu.pixel.filters.distort", None, ITEM),
    (3, "pixel.filters.distort.deform", "menu.pixel.filters.distort.deform", None, ITEM),
    (3, "pixel.filters.distort.perspective", "menu.pixel.filters.distort.perspective", None, ITEM),
    (3, "pixel.filters.distort.twirl", "menu.pixel.filters.distort.twirl", None, ITEM),
    (3, "pixel.filters.distort.pinch_punch", "menu.pixel.filters.distort.pinch_punch", None, ITEM),
    (3, "pixel.filters.distort.spherical", "menu.pixel.filters.distort.spherical", None, ITEM),
    (3, "pixel.filters.distort.ripple", "menu.pixel.filters.distort.ripple", None, ITEM),
    (3, "pixel.filters.distort.lens_distortion", "menu.pixel.filters.distort.lens_distortion", None, ITEM),
    (3, "pixel.filters.distort.lens_correction", "menu.pixel.filters.distort.lens_correction", None, ITEM),
    (3, "pixel.filters.distort.rectangular_to_polar", "menu.pixel.filters.distort.rectangular_to_polar", None, ITEM),
    (3, "pixel.filters.distort.polar_to_rectangular", "menu.pixel.filters.distort.polar_to_rectangular", None, ITEM),
    (3, "pixel.filters.distort.pixelate", "menu.pixel.filters.distort.pixelate", None, ITEM),
    (3, "pixel.filters.distort.displace", "menu.pixel.filters.distort.displace", None, ITEM),
    (3, "pixel.filters.distort.glitch", "menu.pixel.filters.distort.glitch", None, ITEM),
    (3, "pixel.filters.distort.shear", "menu.pixel.filters.distort.shear", None, ITEM),
    (3, "pixel.filters.distort.mirror", "menu.pixel.filters.distort.mirror", None, ITEM),
    (3, "pixel.filters.distort.affine", "menu.pixel.filters.distort.affine", None, ITEM),
    (3, "pixel.filters.distort.equations", "menu.pixel.filters.distort.equations", None, ITEM),
    (3, "pixel.filters.distort.mesh_warp", "menu.pixel.filters.distort.mesh_warp", None, ITEM),
    (2, "pixel.filters.noise", "menu.pixel.filters.noise", None, ITEM),
    (3, "pixel.filters.noise.denoise", "menu.pixel.filters.noise.denoise", None, ITEM),
    (3, "pixel.filters.noise.wavelet_denoise", "menu.pixel.filters.noise.wavelet_denoise", None, ITEM),
    (3, "pixel.filters.noise.add_noise", "menu.pixel.filters.noise.add_noise", None, ITEM),
    (3, "pixel.filters.noise.diffuse", "menu.pixel.filters.noise.diffuse", None, ITEM),
    (3, "pixel.filters.noise.perlin_noise", "menu.pixel.filters.noise.perlin_noise", None, ITEM),
    (3, "pixel.filters.noise.fft_denoise", "menu.pixel.filters.noise.fft_denoise", None, ITEM),
    (3, "pixel.filters.noise.dust_and_scratches", "menu.pixel.filters.noise.dust_and_scratches", None, ITEM),
    (3, "pixel.filters.noise.deinterlace", "menu.pixel.filters.noise.deinterlace", None, ITEM),
    (4, "pixel.filters.noise.deinterlace.even_rows", "menu.pixel.filters.noise.deinterlace.even_rows", None, ITEM),
    (4, "pixel.filters.noise.deinterlace.odd_rows", "menu.pixel.filters.noise.deinterlace.odd_rows", None, ITEM),
    (2, "pixel.filters.detect", "menu.pixel.filters.detect", None, ITEM),
    (3, "pixel.filters.detect.edges", "menu.pixel.filters.detect.edges", None, ITEM),
    (3, "pixel.filters.detect.horizontal_edges", "menu.pixel.filters.detect.horizontal_edges", None, ITEM),
    (3, "pixel.filters.detect.vertical_edges", "menu.pixel.filters.detect.vertical_edges", None, ITEM),
    (2, "pixel.filters.lighting", "menu.pixel.filters.lighting", None, ITEM),
    (3, "pixel.filters.lighting.lighting", "menu.pixel.filters.lighting.lighting", None, ITEM),
    (3, "pixel.filters.lighting.shadows_highlights", "menu.pixel.filters.lighting.shadows_highlights", None, ITEM),
    (2, "pixel.filters.colours", "menu.pixel.filters.colours", None, ITEM),
    (3, "pixel.filters.colours.fill", "menu.pixel.filters.colours.fill", None, ITEM),
    (3, "pixel.filters.colours.fill_with_foreground_colour", "menu.pixel.filters.colours.fill_with_foreground_colour", None, ITEM),
    (3, "pixel.filters.colours.fill_with_background_colour", "menu.pixel.filters.colours.fill_with_background_colour", None, ITEM),
    (3, "pixel.filters.colours.matte", "menu.pixel.filters.colours.matte", None, ITEM),
    (3, "pixel.filters.colours.sep1", "", None, SEP),
    (3, "pixel.filters.colours.auto_levels", "menu.pixel.filters.colours.auto_levels", None, ITEM),
    (3, "pixel.filters.colours.auto_contrast", "menu.pixel.filters.colours.auto_contrast", None, ITEM),
    (3, "pixel.filters.colours.auto_colours", "menu.pixel.filters.colours.auto_colours", None, ITEM),
    (3, "pixel.filters.colours.auto_white_balance", "menu.pixel.filters.colours.auto_white_balance", None, ITEM),
    (3, "pixel.filters.colours.sep2", "", None, SEP),
    (3, "pixel.filters.colours.vignette", "menu.pixel.filters.colours.vignette", None, ITEM),
    (3, "pixel.filters.colours.remove_vignette", "menu.pixel.filters.colours.remove_vignette", None, ITEM),
    (3, "pixel.filters.colours.chromatic_aberration", "menu.pixel.filters.colours.chromatic_aberration", None, ITEM),
    (3, "pixel.filters.colours.defringe", "menu.pixel.filters.colours.defringe", None, ITEM),
    (3, "pixel.filters.colours.remove_white_matte", "menu.pixel.filters.colours.remove_white_matte", None, ITEM),
    (3, "pixel.filters.colours.remove_black_matte", "menu.pixel.filters.colours.remove_black_matte", None, ITEM),
    (3, "pixel.filters.colours.multiply_by_alpha", "menu.pixel.filters.colours.multiply_by_alpha", None, ITEM),
    (3, "pixel.filters.colours.divide_by_alpha", "menu.pixel.filters.colours.divide_by_alpha", None, ITEM),
    (3, "pixel.filters.colours.erase_white_paper", "menu.pixel.filters.colours.erase_white_paper", None, ITEM),
    (3, "pixel.filters.colours.sep3", "", None, SEP),
    (3, "pixel.filters.colours.monochrome_dither", "menu.pixel.filters.colours.monochrome_dither", None, ITEM),
    (3, "pixel.filters.colours.web_safe_dither", "menu.pixel.filters.colours.web_safe_dither", None, ITEM),
    (3, "pixel.filters.colours.emboss", "menu.pixel.filters.colours.emboss", None, ITEM),
    (3, "pixel.filters.colours.solarise", "menu.pixel.filters.colours.solarise", None, ITEM),
    (3, "pixel.filters.colours.halftone", "menu.pixel.filters.colours.halftone", None, ITEM),
    (3, "pixel.filters.colours.voronoi", "menu.pixel.filters.colours.voronoi", None, ITEM),
    (3, "pixel.filters.colours.sep4", "", None, SEP),
    (3, "pixel.filters.colours.procedural_texture", "menu.pixel.filters.colours.procedural_texture", None, ITEM),
    (2, "pixel.filters.astrophotography", "menu.pixel.filters.astrophotography", None, ITEM),
    (3, "pixel.filters.astrophotography.colour_map_mono_layers", "menu.pixel.filters.astrophotography.colour_map_mono_layers", None, ITEM),
    (3, "pixel.filters.astrophotography.linear_fit", "menu.pixel.filters.astrophotography.linear_fit", None, ITEM),
    (3, "pixel.filters.astrophotography.separate_luminosity_and_colour", "menu.pixel.filters.astrophotography.separate_luminosity_and_colour", None, ITEM),
    (3, "pixel.filters.astrophotography.sep1", "", None, SEP),
    (3, "pixel.filters.astrophotography.auto_stretch", "menu.pixel.filters.astrophotography.auto_stretch", None, ITEM),
    (3, "pixel.filters.astrophotography.remove_background", "menu.pixel.filters.astrophotography.remove_background", None, ITEM),
    (3, "pixel.filters.astrophotography.colour_calibration", "menu.pixel.filters.astrophotography.colour_calibration", None, ITEM),
    (3, "pixel.filters.astrophotography.star_and_background_separation", "menu.pixel.filters.astrophotography.star_and_background_separation", None, ITEM),
    (3, "pixel.filters.astrophotography.sep2", "", None, SEP),
    (3, "pixel.filters.astrophotography.enhance_structure", "menu.pixel.filters.astrophotography.enhance_structure", None, ITEM),
    (2, "pixel.filters.sep2", "", None, SEP),
    (2, "pixel.filters.frequency_separation", "menu.pixel.filters.frequency_separation", None, ITEM),
    (2, "pixel.filters.apply_image", "menu.pixel.filters.apply_image", None, ITEM),
    (2, "pixel.filters.haze_removal", "menu.pixel.filters.haze_removal", None, ITEM),
    (2, "pixel.filters.sep3", "", None, SEP),
    (2, "pixel.filters.develop", "menu.pixel.filters.develop", None, ITEM),
    (2, "pixel.filters.liquify", "menu.pixel.filters.liquify", None, ITEM),
    (2, "pixel.filters.tone_map", "menu.pixel.filters.tone_map", None, ITEM),
    (2, "pixel.filters.sep4", "", None, SEP),
    (2, "pixel.filters.plugins", "menu.pixel.filters.plugins", None, ITEM),
    (1, "pixel.sep1", "", None, SEP),
    (1, "pixel.pixel_selection", "menu.pixel.pixel_selection", None, ITEM),
    (2, "pixel.pixel_selection.select_all", "menu.pixel.pixel_selection.select_all", Some("selection.all"), ITEM),
    (2, "pixel.pixel_selection.deselect", "menu.pixel.pixel_selection.deselect", Some("selection.none"), ITEM),
    (2, "pixel.pixel_selection.reselect", "menu.pixel.pixel_selection.reselect", Some("selection.reselect"), ITEM),
    (2, "pixel.pixel_selection.invert", "menu.pixel.pixel_selection.invert", Some("selection.invert"), ITEM),
    (2, "pixel.pixel_selection.sep1", "", None, SEP),
    (2, "pixel.pixel_selection.from_layer", "menu.pixel.pixel_selection.from_layer", Some("selection.fromLayer"), ITEM),
    (2, "pixel.pixel_selection.from_layer_intensity", "menu.pixel.pixel_selection.from_layer_intensity", Some("selection.fromLayerIntensity"), ITEM),
    (2, "pixel.pixel_selection.from_layer_and_delete", "menu.pixel.pixel_selection.from_layer_and_delete", Some("selection.fromLayerAndDelete"), ITEM),
    (2, "pixel.pixel_selection.detected_objects", "menu.pixel.pixel_selection.detected_objects", None, ITEM),
    (3, "pixel.pixel_selection.detected_objects.none_detected", "menu.pixel.pixel_selection.detected_objects.none_detected", None, ITEM),
    (2, "pixel.pixel_selection.detected_regions", "menu.pixel.pixel_selection.detected_regions", None, ITEM),
    (3, "pixel.pixel_selection.detected_regions.none_detected", "menu.pixel.pixel_selection.detected_regions.none_detected", None, ITEM),
    (2, "pixel.pixel_selection.sep2", "", None, SEP),
    (2, "pixel.pixel_selection.save", "menu.pixel.pixel_selection.save", None, ITEM),
    (3, "pixel.pixel_selection.save.as_spare_channel", "menu.selection.save_channel", Some("selection.save"), ITEM),
    (3, "pixel.pixel_selection.save.to_file", "menu.selection.save_file", Some("selection.save"), ITEM),
    (2, "pixel.pixel_selection.load_from_file", "menu.selection.load_file", Some("selection.load"), ITEM),
    (2, "pixel.pixel_selection.load_channel", "menu.selection.load_channel", Some("selection.load"), ITEM),
    (2, "pixel.pixel_selection.sep3", "", None, SEP),
    (2, "pixel.pixel_selection.colour_range", "menu.pixel.pixel_selection.colour_range", None, ITEM),
    (3, "pixel.pixel_selection.colour_range.reds", "menu.pixel.pixel_selection.colour_range.reds", Some("selection.hueRange.reds"), ITEM),
    (3, "pixel.pixel_selection.colour_range.greens", "menu.pixel.pixel_selection.colour_range.greens", Some("selection.hueRange.greens"), ITEM),
    (3, "pixel.pixel_selection.colour_range.blues", "menu.pixel.pixel_selection.colour_range.blues", Some("selection.hueRange.blues"), ITEM),
    (2, "pixel.pixel_selection.tonal_range", "menu.pixel.pixel_selection.tonal_range", None, ITEM),
    (3, "pixel.pixel_selection.tonal_range.midtones", "menu.pixel.pixel_selection.tonal_range.midtones", Some("selection.tonalRange.midtones"), ITEM),
    (3, "pixel.pixel_selection.tonal_range.shadows", "menu.pixel.pixel_selection.tonal_range.shadows", Some("selection.tonalRange.shadows"), ITEM),
    (3, "pixel.pixel_selection.tonal_range.highlights", "menu.pixel.pixel_selection.tonal_range.highlights", Some("selection.tonalRange.highlights"), ITEM),
    (2, "pixel.pixel_selection.alpha_range", "menu.pixel.pixel_selection.alpha_range", None, ITEM),
    (3, "pixel.pixel_selection.alpha_range.fully_transparent", "menu.pixel.pixel_selection.alpha_range.fully_transparent", Some("selection.alphaRange.transparent"), ITEM),
    (3, "pixel.pixel_selection.alpha_range.partially_transparent", "menu.pixel.pixel_selection.alpha_range.partially_transparent", Some("selection.alphaRange.partial"), ITEM),
    (3, "pixel.pixel_selection.alpha_range.opaque", "menu.pixel.pixel_selection.alpha_range.opaque", Some("selection.alphaRange.opaque"), ITEM),
    (2, "pixel.pixel_selection.sep4", "", None, SEP),
    (2, "pixel.pixel_selection.select_sampled_colour", "menu.pixel.pixel_selection.select_sampled_colour", Some("selection.colorRange"), ITEM),
    (2, "pixel.pixel_selection.sep5", "", None, SEP),
    (2, "pixel.pixel_selection.grow_shrink", "menu.pixel.pixel_selection.grow_shrink", Some("selection.modify"), ITEM),
    (2, "pixel.pixel_selection.feather", "menu.pixel.pixel_selection.feather", Some("selection.modify"), ITEM),
    (2, "pixel.pixel_selection.smooth", "menu.pixel.pixel_selection.smooth", Some("selection.modify"), ITEM),
    (2, "pixel.pixel_selection.refine_edges", "menu.pixel.pixel_selection.refine_edges", Some("selection.refine"), ITEM),
    (2, "pixel.pixel_selection.outline", "menu.pixel.pixel_selection.outline", Some("selection.modify"), ITEM),
    (2, "pixel.pixel_selection.sep6", "", None, SEP),
    (2, "pixel.pixel_selection.edit_as_layer", "menu.pixel.pixel_selection.edit_as_layer", Some("selection.quickmask.toggle"), TOG),
    (1, "pixel.sep2", "", None, SEP),
    (1, "pixel.new_layer", "menu.pixel.new_layer", None, ITEM),
    (1, "pixel.new_mask", "menu.pixel.new_mask", None, ITEM),
    (2, "pixel.new_mask.mask_layer", "menu.pixel.new_mask.mask_layer", None, ITEM),
    (2, "pixel.new_mask.empty_mask_layer", "menu.pixel.new_mask.empty_mask_layer", None, ITEM),
    (2, "pixel.new_mask.live_mask_layer", "menu.pixel.new_mask.live_mask_layer", None, ITEM),
    (3, "pixel.new_mask.live_mask_layer.luminosity_range", "menu.pixel.new_mask.live_mask_layer.luminosity_range", None, ITEM),
    (3, "pixel.new_mask.live_mask_layer.hue_range", "menu.pixel.new_mask.live_mask_layer.hue_range", None, ITEM),
    (3, "pixel.new_mask.live_mask_layer.band_pass", "menu.pixel.new_mask.live_mask_layer.band_pass", None, ITEM),
    (2, "pixel.new_mask.compound_mask_layer", "menu.pixel.new_mask.compound_mask_layer", None, ITEM),
    (1, "pixel.new_adjustment_layer", "menu.pixel.new_adjustment_layer", None, ITEM),
    (2, "pixel.new_adjustment_layer.exposure", "menu.pixel.new_adjustment_layer.exposure", None, ITEM),
    (2, "pixel.new_adjustment_layer.levels", "menu.pixel.new_adjustment_layer.levels", None, ITEM),
    (2, "pixel.new_adjustment_layer.white_balance", "menu.pixel.new_adjustment_layer.white_balance", None, ITEM),
    (2, "pixel.new_adjustment_layer.brightness_contrast", "menu.pixel.new_adjustment_layer.brightness_contrast", None, ITEM),
    (2, "pixel.new_adjustment_layer.sep1", "", None, SEP),
    (2, "pixel.new_adjustment_layer.curves", "menu.pixel.new_adjustment_layer.curves", None, ITEM),
    (2, "pixel.new_adjustment_layer.saturation_hue_curves", "menu.pixel.new_adjustment_layer.saturation_hue_curves", None, ITEM),
    (2, "pixel.new_adjustment_layer.channel_mixer", "menu.pixel.new_adjustment_layer.channel_mixer", None, ITEM),
    (2, "pixel.new_adjustment_layer.selective_colour", "menu.pixel.new_adjustment_layer.selective_colour", None, ITEM),
    (2, "pixel.new_adjustment_layer.colour_balance", "menu.pixel.new_adjustment_layer.colour_balance", None, ITEM),
    (2, "pixel.new_adjustment_layer.gradient_map", "menu.pixel.new_adjustment_layer.gradient_map", None, ITEM),
    (2, "pixel.new_adjustment_layer.black_and_white", "menu.pixel.new_adjustment_layer.black_and_white", None, ITEM),
    (2, "pixel.new_adjustment_layer.hsl", "menu.pixel.new_adjustment_layer.hsl", None, ITEM),
    (2, "pixel.new_adjustment_layer.recolour", "menu.pixel.new_adjustment_layer.recolour", None, ITEM),
    (2, "pixel.new_adjustment_layer.posterise", "menu.pixel.new_adjustment_layer.posterise", None, ITEM),
    (2, "pixel.new_adjustment_layer.vibrance", "menu.pixel.new_adjustment_layer.vibrance", None, ITEM),
    (2, "pixel.new_adjustment_layer.soft_proof", "menu.pixel.new_adjustment_layer.soft_proof", None, ITEM),
    (2, "pixel.new_adjustment_layer.lut", "menu.pixel.new_adjustment_layer.lut", None, ITEM),
    (2, "pixel.new_adjustment_layer.lens_filter", "menu.pixel.new_adjustment_layer.lens_filter", None, ITEM),
    (2, "pixel.new_adjustment_layer.split_toning", "menu.pixel.new_adjustment_layer.split_toning", None, ITEM),
    (2, "pixel.new_adjustment_layer.ocio", "menu.pixel.new_adjustment_layer.ocio", None, ITEM),
    (2, "pixel.new_adjustment_layer.normals", "menu.pixel.new_adjustment_layer.normals", None, ITEM),
    (2, "pixel.new_adjustment_layer.tone_compression", "menu.pixel.new_adjustment_layer.tone_compression", None, ITEM),
    (2, "pixel.new_adjustment_layer.tone_stretch", "menu.pixel.new_adjustment_layer.tone_stretch", None, ITEM),
    (2, "pixel.new_adjustment_layer.sep2", "", None, SEP),
    (2, "pixel.new_adjustment_layer.invert", "menu.pixel.new_adjustment_layer.invert", None, ITEM),
    (2, "pixel.new_adjustment_layer.threshold", "menu.pixel.new_adjustment_layer.threshold", None, ITEM),
    (1, "pixel.new_live_filter_layer", "menu.pixel.new_live_filter_layer", None, ITEM),
    (2, "pixel.new_live_filter_layer.blur", "menu.pixel.new_live_filter_layer.blur", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.gaussian", "menu.pixel.new_live_filter_layer.blur.gaussian", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.diffuse_glow", "menu.pixel.new_live_filter_layer.blur.diffuse_glow", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.bloom", "menu.pixel.new_live_filter_layer.blur.bloom", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.box", "menu.pixel.new_live_filter_layer.blur.box", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.motion", "menu.pixel.new_live_filter_layer.blur.motion", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.radial", "menu.pixel.new_live_filter_layer.blur.radial", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.lens", "menu.pixel.new_live_filter_layer.blur.lens", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.depth_of_field", "menu.pixel.new_live_filter_layer.blur.depth_of_field", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.field", "menu.pixel.new_live_filter_layer.blur.field", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.sep1", "", None, SEP),
    (3, "pixel.new_live_filter_layer.blur.maximum", "menu.pixel.new_live_filter_layer.blur.maximum", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.minimum", "menu.pixel.new_live_filter_layer.blur.minimum", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.sep2", "", None, SEP),
    (3, "pixel.new_live_filter_layer.blur.median", "menu.pixel.new_live_filter_layer.blur.median", None, ITEM),
    (3, "pixel.new_live_filter_layer.blur.bilateral", "menu.pixel.new_live_filter_layer.blur.bilateral", None, ITEM),
    (2, "pixel.new_live_filter_layer.sharpen", "menu.pixel.new_live_filter_layer.sharpen", None, ITEM),
    (3, "pixel.new_live_filter_layer.sharpen.unsharp_mask", "menu.pixel.new_live_filter_layer.sharpen.unsharp_mask", None, ITEM),
    (3, "pixel.new_live_filter_layer.sharpen.clarity", "menu.pixel.new_live_filter_layer.sharpen.clarity", None, ITEM),
    (3, "pixel.new_live_filter_layer.sharpen.texture", "menu.pixel.new_live_filter_layer.sharpen.texture", None, ITEM),
    (3, "pixel.new_live_filter_layer.sharpen.high_pass", "menu.pixel.new_live_filter_layer.sharpen.high_pass", None, ITEM),
    (3, "pixel.new_live_filter_layer.sharpen.multi_band", "menu.pixel.new_live_filter_layer.sharpen.multi_band", None, ITEM),
    (2, "pixel.new_live_filter_layer.distort", "menu.pixel.new_live_filter_layer.distort", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.perspective", "menu.pixel.new_live_filter_layer.distort.perspective", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.twirl", "menu.pixel.new_live_filter_layer.distort.twirl", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.pinch_punch", "menu.pixel.new_live_filter_layer.distort.pinch_punch", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.spherical", "menu.pixel.new_live_filter_layer.distort.spherical", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.ripple", "menu.pixel.new_live_filter_layer.distort.ripple", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.lens_distortion", "menu.pixel.new_live_filter_layer.distort.lens_distortion", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.pixelate", "menu.pixel.new_live_filter_layer.distort.pixelate", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.displace", "menu.pixel.new_live_filter_layer.distort.displace", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.glitch", "menu.pixel.new_live_filter_layer.distort.glitch", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.mesh_warp", "menu.pixel.new_live_filter_layer.distort.mesh_warp", None, ITEM),
    (3, "pixel.new_live_filter_layer.distort.liquify", "menu.pixel.new_live_filter_layer.distort.liquify", None, ITEM),
    (2, "pixel.new_live_filter_layer.noise", "menu.pixel.new_live_filter_layer.noise", None, ITEM),
    (3, "pixel.new_live_filter_layer.noise.denoise", "menu.pixel.new_live_filter_layer.noise.denoise", None, ITEM),
    (3, "pixel.new_live_filter_layer.noise.wavelet_denoise", "menu.pixel.new_live_filter_layer.noise.wavelet_denoise", None, ITEM),
    (3, "pixel.new_live_filter_layer.noise.add_noise", "menu.pixel.new_live_filter_layer.noise.add_noise", None, ITEM),
    (3, "pixel.new_live_filter_layer.noise.diffuse", "menu.pixel.new_live_filter_layer.noise.diffuse", None, ITEM),
    (3, "pixel.new_live_filter_layer.noise.dust_and_scratches", "menu.pixel.new_live_filter_layer.noise.dust_and_scratches", None, ITEM),
    (2, "pixel.new_live_filter_layer.lighting", "menu.pixel.new_live_filter_layer.lighting", None, ITEM),
    (3, "pixel.new_live_filter_layer.lighting.lighting", "menu.pixel.new_live_filter_layer.lighting.lighting", None, ITEM),
    (3, "pixel.new_live_filter_layer.lighting.shadows_highlights", "menu.pixel.new_live_filter_layer.lighting.shadows_highlights", None, ITEM),
    (2, "pixel.new_live_filter_layer.colours", "menu.pixel.new_live_filter_layer.colours", None, ITEM),
    (3, "pixel.new_live_filter_layer.colours.vignette", "menu.pixel.new_live_filter_layer.colours.vignette", None, ITEM),
    (3, "pixel.new_live_filter_layer.colours.defringe", "menu.pixel.new_live_filter_layer.colours.defringe", None, ITEM),
    (3, "pixel.new_live_filter_layer.colours.sep1", "", None, SEP),
    (3, "pixel.new_live_filter_layer.colours.halftone", "menu.pixel.new_live_filter_layer.colours.halftone", None, ITEM),
    (3, "pixel.new_live_filter_layer.colours.voronoi", "menu.pixel.new_live_filter_layer.colours.voronoi", None, ITEM),
    (3, "pixel.new_live_filter_layer.colours.sep2", "", None, SEP),
    (3, "pixel.new_live_filter_layer.colours.procedural_texture", "menu.pixel.new_live_filter_layer.colours.procedural_texture", None, ITEM),
    (1, "pixel.new_fill_layer", "menu.pixel.new_fill_layer", None, ITEM),
    (1, "pixel.new_pattern_layer", "menu.pixel.new_pattern_layer", None, ITEM),
    (1, "pixel.new_pattern_layer_from_selection", "menu.pixel.new_pattern_layer_from_selection", None, ITEM),
    (1, "pixel.sep3", "", None, SEP),
    (1, "pixel.invert", "menu.pixel.invert", None, ITEM),
    (1, "pixel.merge", "menu.pixel.merge", None, ITEM),
    (2, "pixel.merge.down", "menu.pixel.merge.down", None, ITEM),
    (2, "pixel.merge.selected", "menu.pixel.merge.selected", None, ITEM),
    (2, "pixel.merge.visible", "menu.pixel.merge.visible", None, ITEM),
    (1, "pixel.refine_mask", "menu.pixel.refine_mask", None, ITEM),
    (1, "pixel.sep4", "", None, SEP),
    (1, "pixel.live_stack_group", "menu.pixel.live_stack_group", None, ITEM),
    (1, "pixel.live_tone_blend_group", "menu.pixel.live_tone_blend_group", Some("toneblend.create"), ITEM),
    (1, "pixel.live_projection", "menu.pixel.live_projection", None, ITEM),
    (2, "pixel.live_projection.remove_projection", "menu.pixel.live_projection.remove_projection", None, ITEM),
    (2, "pixel.live_projection.edit_live_projection", "menu.pixel.live_projection.edit_live_projection", None, ITEM),
    (2, "pixel.live_projection.sep1", "", None, SEP),
    (2, "pixel.live_projection.equirectangular", "menu.pixel.live_projection.equirectangular", None, ITEM),
    (2, "pixel.live_projection.perspective", "menu.pixel.live_projection.perspective", None, ITEM),
    (1, "pixel.sep5", "", None, SEP),
    (1, "pixel.rasterise", "menu.pixel.rasterise", None, ITEM),
    (2, "pixel.rasterise.rasterise_and_trim", "menu.pixel.rasterise.rasterise_and_trim", Some("layer.rasterise.trim"), ITEM),
    (2, "pixel.rasterise.rasterise", "menu.pixel.rasterise.rasterise", Some("layer.rasterise"), ITEM),
    (2, "pixel.rasterise.rasterise_to_mask", "menu.pixel.rasterise.rasterise_to_mask", None, ITEM),
    (1, "pixel.inpaint", "menu.pixel.inpaint", None, ITEM),
    (0, "layer", "menu.layer", None, ITEM),
    (1, "layer.select", "menu.layer.select", None, ITEM),
    (2, "layer.select.next_layer", "menu.layer.select.next_layer", None, ITEM),
    (2, "layer.select.previous_layer", "menu.layer.select.previous_layer", None, ITEM),
    (2, "layer.select.parent_layer", "menu.layer.select.parent_layer", None, ITEM),
    (2, "layer.select.top_layer", "menu.layer.select.top_layer", None, ITEM),
    (2, "layer.select.bottom_layer", "menu.layer.select.bottom_layer", None, ITEM),
    (2, "layer.select.sep1", "", None, SEP),
    (2, "layer.select.all_layers", "menu.layer.select.all_layers", None, ITEM),
    (2, "layer.select.all_on_current_layer", "menu.layer.select.all_on_current_layer", None, ITEM),
    (2, "layer.select.deselect_layers", "menu.layer.select.deselect_layers", None, ITEM),
    (2, "layer.select.sep2", "", None, SEP),
    (2, "layer.select.cycle_selection_box", "menu.layer.select.cycle_selection_box", None, ITEM),
    (2, "layer.select.set_selection_box", "menu.layer.select.set_selection_box", None, ITEM),
    (2, "layer.select.sep3", "", None, SEP),
    (2, "layer.select.same", "menu.layer.select.same", None, ITEM),
    (3, "layer.select.same.fill_colour", "menu.layer.select.same.fill_colour", None, ITEM),
    (3, "layer.select.same.stroke_colour", "menu.layer.select.same.stroke_colour", None, ITEM),
    (3, "layer.select.same.fill_and_stroke_colour", "menu.layer.select.same.fill_and_stroke_colour", None, ITEM),
    (3, "layer.select.same.sep1", "", None, SEP),
    (3, "layer.select.same.stroke_weight", "menu.layer.select.same.stroke_weight", None, ITEM),
    (4, "layer.select.same.stroke_weight.less", "menu.layer.select.same.stroke_weight.less", None, ITEM),
    (4, "layer.select.same.stroke_weight.less_or_equal", "menu.layer.select.same.stroke_weight.less_or_equal", None, ITEM),
    (4, "layer.select.same.stroke_weight.equal", "menu.layer.select.same.stroke_weight.equal", None, ITEM),
    (4, "layer.select.same.stroke_weight.greater", "menu.layer.select.same.stroke_weight.greater", None, ITEM),
    (4, "layer.select.same.stroke_weight.greater_or_equal", "menu.layer.select.same.stroke_weight.greater_or_equal", None, ITEM),
    (3, "layer.select.same.transparency", "menu.layer.select.same.transparency", None, ITEM),
    (3, "layer.select.same.blend_mode", "menu.layer.select.same.blend_mode", None, ITEM),
    (3, "layer.select.same.shape", "menu.layer.select.same.shape", None, ITEM),
    (3, "layer.select.same.sep2", "", None, SEP),
    (3, "layer.select.same.width", "menu.layer.select.same.width", None, ITEM),
    (4, "layer.select.same.width.less", "menu.layer.select.same.width.less", None, ITEM),
    (4, "layer.select.same.width.less_or_equal", "menu.layer.select.same.width.less_or_equal", None, ITEM),
    (4, "layer.select.same.width.equal", "menu.layer.select.same.width.equal", None, ITEM),
    (4, "layer.select.same.width.greater", "menu.layer.select.same.width.greater", None, ITEM),
    (4, "layer.select.same.width.greater_or_equal", "menu.layer.select.same.width.greater_or_equal", None, ITEM),
    (3, "layer.select.same.height", "menu.layer.select.same.height", None, ITEM),
    (4, "layer.select.same.height.less", "menu.layer.select.same.height.less", None, ITEM),
    (4, "layer.select.same.height.less_or_equal", "menu.layer.select.same.height.less_or_equal", None, ITEM),
    (4, "layer.select.same.height.equal", "menu.layer.select.same.height.equal", None, ITEM),
    (4, "layer.select.same.height.greater", "menu.layer.select.same.height.greater", None, ITEM),
    (4, "layer.select.same.height.greater_or_equal", "menu.layer.select.same.height.greater_or_equal", None, ITEM),
    (3, "layer.select.same.rotation", "menu.layer.select.same.rotation", None, ITEM),
    (3, "layer.select.same.sep3", "", None, SEP),
    (3, "layer.select.same.name", "menu.layer.select.same.name", Some(dtool_core::commands::selection_edit::EDIT_SELECT_SAME), ITEM),
    (3, "layer.select.same.tag_colour", "menu.layer.select.same.tag_colour", Some(dtool_core::commands::selection_edit::EDIT_SELECT_SAME), ITEM),
    (3, "layer.select.same.sep4", "", None, SEP),
    (3, "layer.select.same.select_hidden_objects", "menu.layer.select.same.select_hidden_objects", None, TOG),
    (2, "layer.select.layer", "menu.layer.select.layer", None, ITEM),
    (3, "layer.select.layer.new", "menu.layer.select.layer.new", None, ITEM),
    (4, "layer.select.layer.new.art_text", "menu.layer.select.layer.new.art_text", None, ITEM),
    (4, "layer.select.layer.new.frame_text", "menu.layer.select.layer.new.frame_text", None, ITEM),
    (4, "layer.select.layer.new.path_text", "menu.layer.select.layer.new.path_text", None, ITEM),
    (4, "layer.select.layer.new.tables", "menu.layer.select.layer.new.tables", None, ITEM),
    (4, "layer.select.layer.new.sep1", "", None, SEP),
    (4, "layer.select.layer.new.curves", "menu.layer.select.layer.new.curves", None, ITEM),
    (4, "layer.select.layer.new.open_curves", "menu.layer.select.layer.new.open_curves", None, ITEM),
    (4, "layer.select.layer.new.closed_curves", "menu.layer.select.layer.new.closed_curves", None, ITEM),
    (4, "layer.select.layer.new.sep2", "", None, SEP),
    (4, "layer.select.layer.new.shapes", "menu.layer.select.layer.new.shapes", None, ITEM),
    (4, "layer.select.layer.new.artboards", "menu.layer.select.layer.new.artboards", None, ITEM),
    (4, "layer.select.layer.new.picture_frames", "menu.layer.select.layer.new.picture_frames", None, ITEM),
    (4, "layer.select.layer.new.data_merge_layouts", "menu.layer.select.layer.new.data_merge_layouts", None, ITEM),
    (4, "layer.select.layer.new.sep3", "", None, SEP),
    (4, "layer.select.layer.new.filled_objects", "menu.layer.select.layer.new.filled_objects", None, ITEM),
    (4, "layer.select.layer.new.unfilled_objects", "menu.layer.select.layer.new.unfilled_objects", None, ITEM),
    (4, "layer.select.layer.new.stroked_objects", "menu.layer.select.layer.new.stroked_objects", None, ITEM),
    (4, "layer.select.layer.new.unstroked_objects", "menu.layer.select.layer.new.unstroked_objects", None, ITEM),
    (4, "layer.select.layer.new.dashed_objects", "menu.layer.select.layer.new.dashed_objects", None, ITEM),
    (4, "layer.select.layer.new.undashed_objects", "menu.layer.select.layer.new.undashed_objects", None, ITEM),
    (4, "layer.select.layer.new.constrained_objects", "menu.layer.select.layer.new.constrained_objects", None, ITEM),
    (4, "layer.select.layer.new.transparent_objects", "menu.layer.select.layer.new.transparent_objects", None, ITEM),
    (4, "layer.select.layer.new.opaque_objects", "menu.layer.select.layer.new.opaque_objects", None, ITEM),
    (4, "layer.select.layer.new.sep4", "", None, SEP),
    (4, "layer.select.layer.new.images", "menu.layer.select.layer.new.images", None, ITEM),
    (4, "layer.select.layer.new.pixel_objects", "menu.layer.select.layer.new.pixel_objects", None, ITEM),
    (4, "layer.select.layer.new.sep5", "", None, SEP),
    (4, "layer.select.layer.new.groups", "menu.layer.select.layer.new.groups", None, ITEM),
    (4, "layer.select.layer.new.container_layers", "menu.layer.select.layer.new.container_layers", None, ITEM),
    (4, "layer.select.layer.new.embedded_documents", "menu.layer.select.layer.new.embedded_documents", None, ITEM),
    (4, "layer.select.layer.new.symbols", "menu.layer.select.layer.new.symbols", None, ITEM),
    (3, "layer.select.layer.add", "menu.layer.select.layer.add", None, ITEM),
    (4, "layer.select.layer.add.art_text", "menu.layer.select.layer.add.art_text", None, ITEM),
    (4, "layer.select.layer.add.frame_text", "menu.layer.select.layer.add.frame_text", None, ITEM),
    (4, "layer.select.layer.add.path_text", "menu.layer.select.layer.add.path_text", None, ITEM),
    (4, "layer.select.layer.add.tables", "menu.layer.select.layer.add.tables", None, ITEM),
    (4, "layer.select.layer.add.sep1", "", None, SEP),
    (4, "layer.select.layer.add.curves", "menu.layer.select.layer.add.curves", None, ITEM),
    (4, "layer.select.layer.add.open_curves", "menu.layer.select.layer.add.open_curves", None, ITEM),
    (4, "layer.select.layer.add.closed_curves", "menu.layer.select.layer.add.closed_curves", None, ITEM),
    (4, "layer.select.layer.add.sep2", "", None, SEP),
    (4, "layer.select.layer.add.shapes", "menu.layer.select.layer.add.shapes", None, ITEM),
    (4, "layer.select.layer.add.artboards", "menu.layer.select.layer.add.artboards", None, ITEM),
    (4, "layer.select.layer.add.picture_frames", "menu.layer.select.layer.add.picture_frames", None, ITEM),
    (4, "layer.select.layer.add.data_merge_layouts", "menu.layer.select.layer.add.data_merge_layouts", None, ITEM),
    (4, "layer.select.layer.add.sep3", "", None, SEP),
    (4, "layer.select.layer.add.filled_objects", "menu.layer.select.layer.add.filled_objects", None, ITEM),
    (4, "layer.select.layer.add.unfilled_objects", "menu.layer.select.layer.add.unfilled_objects", None, ITEM),
    (4, "layer.select.layer.add.stroked_objects", "menu.layer.select.layer.add.stroked_objects", None, ITEM),
    (4, "layer.select.layer.add.unstroked_objects", "menu.layer.select.layer.add.unstroked_objects", None, ITEM),
    (4, "layer.select.layer.add.dashed_objects", "menu.layer.select.layer.add.dashed_objects", None, ITEM),
    (4, "layer.select.layer.add.undashed_objects", "menu.layer.select.layer.add.undashed_objects", None, ITEM),
    (4, "layer.select.layer.add.constrained_objects", "menu.layer.select.layer.add.constrained_objects", None, ITEM),
    (4, "layer.select.layer.add.transparent_objects", "menu.layer.select.layer.add.transparent_objects", None, ITEM),
    (4, "layer.select.layer.add.opaque_objects", "menu.layer.select.layer.add.opaque_objects", None, ITEM),
    (4, "layer.select.layer.add.sep4", "", None, SEP),
    (4, "layer.select.layer.add.images", "menu.layer.select.layer.add.images", None, ITEM),
    (4, "layer.select.layer.add.pixel_objects", "menu.layer.select.layer.add.pixel_objects", None, ITEM),
    (4, "layer.select.layer.add.sep5", "", None, SEP),
    (4, "layer.select.layer.add.groups", "menu.layer.select.layer.add.groups", None, ITEM),
    (4, "layer.select.layer.add.container_layers", "menu.layer.select.layer.add.container_layers", None, ITEM),
    (4, "layer.select.layer.add.embedded_documents", "menu.layer.select.layer.add.embedded_documents", None, ITEM),
    (4, "layer.select.layer.add.symbols", "menu.layer.select.layer.add.symbols", None, ITEM),
    (3, "layer.select.layer.remove", "menu.layer.select.layer.remove", None, ITEM),
    (4, "layer.select.layer.remove.art_text", "menu.layer.select.layer.remove.art_text", None, ITEM),
    (4, "layer.select.layer.remove.frame_text", "menu.layer.select.layer.remove.frame_text", None, ITEM),
    (4, "layer.select.layer.remove.path_text", "menu.layer.select.layer.remove.path_text", None, ITEM),
    (4, "layer.select.layer.remove.tables", "menu.layer.select.layer.remove.tables", None, ITEM),
    (4, "layer.select.layer.remove.sep1", "", None, SEP),
    (4, "layer.select.layer.remove.curves", "menu.layer.select.layer.remove.curves", None, ITEM),
    (4, "layer.select.layer.remove.open_curves", "menu.layer.select.layer.remove.open_curves", None, ITEM),
    (4, "layer.select.layer.remove.closed_curves", "menu.layer.select.layer.remove.closed_curves", None, ITEM),
    (4, "layer.select.layer.remove.sep2", "", None, SEP),
    (4, "layer.select.layer.remove.shapes", "menu.layer.select.layer.remove.shapes", None, ITEM),
    (4, "layer.select.layer.remove.artboards", "menu.layer.select.layer.remove.artboards", None, ITEM),
    (4, "layer.select.layer.remove.picture_frames", "menu.layer.select.layer.remove.picture_frames", None, ITEM),
    (4, "layer.select.layer.remove.data_merge_layouts", "menu.layer.select.layer.remove.data_merge_layouts", None, ITEM),
    (4, "layer.select.layer.remove.sep3", "", None, SEP),
    (4, "layer.select.layer.remove.filled_objects", "menu.layer.select.layer.remove.filled_objects", None, ITEM),
    (4, "layer.select.layer.remove.unfilled_objects", "menu.layer.select.layer.remove.unfilled_objects", None, ITEM),
    (4, "layer.select.layer.remove.stroked_objects", "menu.layer.select.layer.remove.stroked_objects", None, ITEM),
    (4, "layer.select.layer.remove.unstroked_objects", "menu.layer.select.layer.remove.unstroked_objects", None, ITEM),
    (4, "layer.select.layer.remove.dashed_objects", "menu.layer.select.layer.remove.dashed_objects", None, ITEM),
    (4, "layer.select.layer.remove.undashed_objects", "menu.layer.select.layer.remove.undashed_objects", None, ITEM),
    (4, "layer.select.layer.remove.constrained_objects", "menu.layer.select.layer.remove.constrained_objects", None, ITEM),
    (4, "layer.select.layer.remove.transparent_objects", "menu.layer.select.layer.remove.transparent_objects", None, ITEM),
    (4, "layer.select.layer.remove.opaque_objects", "menu.layer.select.layer.remove.opaque_objects", None, ITEM),
    (4, "layer.select.layer.remove.sep4", "", None, SEP),
    (4, "layer.select.layer.remove.images", "menu.layer.select.layer.remove.images", None, ITEM),
    (4, "layer.select.layer.remove.pixel_objects", "menu.layer.select.layer.remove.pixel_objects", None, ITEM),
    (4, "layer.select.layer.remove.sep5", "", None, SEP),
    (4, "layer.select.layer.remove.groups", "menu.layer.select.layer.remove.groups", None, ITEM),
    (4, "layer.select.layer.remove.container_layers", "menu.layer.select.layer.remove.container_layers", None, ITEM),
    (4, "layer.select.layer.remove.embedded_documents", "menu.layer.select.layer.remove.embedded_documents", None, ITEM),
    (4, "layer.select.layer.remove.symbols", "menu.layer.select.layer.remove.symbols", None, ITEM),
    (3, "layer.select.layer.subselect", "menu.layer.select.layer.subselect", None, ITEM),
    (4, "layer.select.layer.subselect.art_text", "menu.layer.select.layer.subselect.art_text", None, ITEM),
    (4, "layer.select.layer.subselect.frame_text", "menu.layer.select.layer.subselect.frame_text", None, ITEM),
    (4, "layer.select.layer.subselect.path_text", "menu.layer.select.layer.subselect.path_text", None, ITEM),
    (4, "layer.select.layer.subselect.tables", "menu.layer.select.layer.subselect.tables", None, ITEM),
    (4, "layer.select.layer.subselect.sep1", "", None, SEP),
    (4, "layer.select.layer.subselect.curves", "menu.layer.select.layer.subselect.curves", None, ITEM),
    (4, "layer.select.layer.subselect.open_curves", "menu.layer.select.layer.subselect.open_curves", None, ITEM),
    (4, "layer.select.layer.subselect.closed_curves", "menu.layer.select.layer.subselect.closed_curves", None, ITEM),
    (4, "layer.select.layer.subselect.sep2", "", None, SEP),
    (4, "layer.select.layer.subselect.shapes", "menu.layer.select.layer.subselect.shapes", None, ITEM),
    (4, "layer.select.layer.subselect.artboards", "menu.layer.select.layer.subselect.artboards", None, ITEM),
    (4, "layer.select.layer.subselect.picture_frames", "menu.layer.select.layer.subselect.picture_frames", None, ITEM),
    (4, "layer.select.layer.subselect.data_merge_layouts", "menu.layer.select.layer.subselect.data_merge_layouts", None, ITEM),
    (4, "layer.select.layer.subselect.sep3", "", None, SEP),
    (4, "layer.select.layer.subselect.filled_objects", "menu.layer.select.layer.subselect.filled_objects", None, ITEM),
    (4, "layer.select.layer.subselect.unfilled_objects", "menu.layer.select.layer.subselect.unfilled_objects", None, ITEM),
    (4, "layer.select.layer.subselect.stroked_objects", "menu.layer.select.layer.subselect.stroked_objects", None, ITEM),
    (4, "layer.select.layer.subselect.unstroked_objects", "menu.layer.select.layer.subselect.unstroked_objects", None, ITEM),
    (4, "layer.select.layer.subselect.dashed_objects", "menu.layer.select.layer.subselect.dashed_objects", None, ITEM),
    (4, "layer.select.layer.subselect.undashed_objects", "menu.layer.select.layer.subselect.undashed_objects", None, ITEM),
    (4, "layer.select.layer.subselect.constrained_objects", "menu.layer.select.layer.subselect.constrained_objects", None, ITEM),
    (4, "layer.select.layer.subselect.transparent_objects", "menu.layer.select.layer.subselect.transparent_objects", None, ITEM),
    (4, "layer.select.layer.subselect.opaque_objects", "menu.layer.select.layer.subselect.opaque_objects", None, ITEM),
    (4, "layer.select.layer.subselect.sep4", "", None, SEP),
    (4, "layer.select.layer.subselect.images", "menu.layer.select.layer.subselect.images", None, ITEM),
    (4, "layer.select.layer.subselect.pixel_objects", "menu.layer.select.layer.subselect.pixel_objects", None, ITEM),
    (4, "layer.select.layer.subselect.sep5", "", None, SEP),
    (4, "layer.select.layer.subselect.groups", "menu.layer.select.layer.subselect.groups", None, ITEM),
    (4, "layer.select.layer.subselect.container_layers", "menu.layer.select.layer.subselect.container_layers", None, ITEM),
    (4, "layer.select.layer.subselect.embedded_documents", "menu.layer.select.layer.subselect.embedded_documents", None, ITEM),
    (4, "layer.select.layer.subselect.symbols", "menu.layer.select.layer.subselect.symbols", None, ITEM),
    (3, "layer.select.layer.sep1", "", None, SEP),
    (3, "layer.select.layer.select_hidden_objects", "menu.layer.select.layer.select_hidden_objects", None, TOG),
    (1, "layer.sep1", "", None, SEP),
    (1, LAYER_EFFECTS_MENU_ID, "menu.layer.layer_effects", None, ITEM),
    (1, "layer.blend_options", "menu.layer.blend_options", None, ITEM),
    (1, "layer.convert", "menu.layer.convert", None, ITEM),
    (2, "layer.convert.to_text_frame", "menu.layer.convert_frame_text", None, ITEM),
    (2, "layer.convert.to_art_text", "menu.layer.convert_art_text", None, ITEM),
    (2, "layer.convert.to_text_path", "menu.layer.convert.to_text_path", None, ITEM),
    (2, "layer.convert.to_picture_frame", "menu.layer.convert.to_picture_frame", Some("frame.convert"), ITEM),
    (2, "layer.convert.to_image_resource", "menu.layer.convert.to_image_resource", None, ITEM),
    (1, "layer.sep2", "", None, SEP),
    (1, "layer.arrange", "menu.layer.arrange", None, ITEM),
    (2, "layer.arrange.move", "menu.layer.arrange.move", None, ITEM),
    (3, "layer.arrange.move.to_front", "menu.layer.arrange.move.to_front", None, ITEM),
    (3, "layer.arrange.move.forward_one", "menu.layer.arrange.move.forward_one", None, ITEM),
    (3, "layer.arrange.move.back_one", "menu.layer.arrange.move.back_one", None, ITEM),
    (3, "layer.arrange.move.to_back", "menu.layer.arrange.move.to_back", None, ITEM),
    (3, "layer.arrange.move.sep1", "", None, SEP),
    (3, "layer.arrange.move.inside", "menu.layer.arrange.move.inside", None, ITEM),
    (3, "layer.arrange.move.outside", "menu.layer.arrange.move.outside", None, ITEM),
    (2, "layer.arrange.align", "menu.layer.arrange.align", None, ITEM),
    (3, "layer.arrange.align.left", "menu.layer.arrange.align.left", None, ITEM),
    (3, "layer.arrange.align.centre", "menu.layer.arrange.align.centre", None, ITEM),
    (3, "layer.arrange.align.right", "menu.layer.arrange.align.right", None, ITEM),
    (3, "layer.arrange.align.sep1", "", None, SEP),
    (3, "layer.arrange.align.top", "menu.layer.arrange.align.top", None, ITEM),
    (3, "layer.arrange.align.middle", "menu.layer.arrange.align.middle", None, ITEM),
    (3, "layer.arrange.align.bottom", "menu.layer.arrange.align.bottom", None, ITEM),
    (3, "layer.arrange.align.sep2", "", None, SEP),
    (3, "layer.arrange.align.space_horizontally", "menu.layer.arrange.align.space_horizontally", Some("layer.arrange.distribute.h"), ITEM),
    (3, "layer.arrange.align.space_vertically", "menu.layer.arrange.align.space_vertically", Some("layer.arrange.distribute.v"), ITEM),
    (3, "layer.arrange.align.sep3", "", None, SEP),
    (3, "layer.arrange.align.distribute_horizontally", "menu.layer.arrange.align.distribute_horizontally", Some("layer.arrange.distribute.h"), ITEM),
    (3, "layer.arrange.align.distribute_vertically", "menu.layer.arrange.align.distribute_vertically", Some("layer.arrange.distribute.v"), ITEM),
    (3, "layer.arrange.align.sep4", "", None, SEP),
    (3, "layer.arrange.align.align_layers_by_stars", "menu.layer.arrange.align.align_layers_by_stars", None, ITEM),
    (2, "layer.arrange.insert", "menu.layer.arrange.insert", None, ITEM),
    (3, "layer.arrange.insert.default", "menu.layer.arrange.insert.default", None, TOG),
    (3, "layer.arrange.insert.behind", "menu.layer.arrange.insert.behind", None, TOG),
    (3, "layer.arrange.insert.top", "menu.layer.arrange.insert.top", None, TOG),
    (3, "layer.arrange.insert.inside", "menu.layer.arrange.insert.inside", None, TOG),
    (2, "layer.arrange.sep1", "", None, SEP),
    (2, "layer.arrange.flip_horizontal", "menu.layer.arrange.flip_horizontal", None, ITEM),
    (2, "layer.arrange.flip_vertical", "menu.layer.arrange.flip_vertical", None, ITEM),
    (2, "layer.arrange.sep2", "", None, SEP),
    (2, "layer.arrange.rotate_right", "menu.layer.arrange.rotate_right", None, ITEM),
    (2, "layer.arrange.rotate_left", "menu.layer.arrange.rotate_left", None, ITEM),
    (1, "layer.sep3", "", None, SEP),
    (1, RENAME_LAYER_MENU_ID, "menu.layer.rename_layer", None, ITEM),
    (1, "layer.sep4", "", None, SEP),
    (1, "layer.create_clipping_mask", "menu.layer.create_clipping_mask", Some(dtool_core::clip_mask::VECTOR_MASK_APPLY_BELOW), ITEM),
    (1, "layer.sep5", "", None, SEP),
    (1, "layer.group", "menu.layer.group", None, ITEM),
    (1, "layer.ungroup", "menu.layer.ungroup", None, ITEM),
    (1, "layer.ungroup_all", "menu.layer.ungroup_all", None, ITEM),
    (1, "layer.new_group", "menu.layer.new_group", None, ITEM),
    (1, "layer.new_folder", "menu.layer.new_folder", None, ITEM),
    (1, "layer.sep6", "", None, SEP),
    (1, "layer.lock", "menu.layer.lock", None, ITEM),
    (1, "layer.unlock", "menu.layer.unlock", None, ITEM),
    (1, "layer.unlock_all", "menu.layer.unlock_all", None, ITEM),
    (1, "layer.sep7", "", None, SEP),
    (1, "layer.master_page", "menu.layer.master_page", None, ITEM),
    (2, "layer.master_page.edit_detached", "menu.layer.master_page.edit_detached", None, TOG),
    (2, "layer.master_page.edit_linked", "menu.layer.master_page.edit_linked", None, TOG),
    (2, "layer.master_page.edit_frame_content", "menu.layer.master_page.edit_frame_content", None, TOG),
    (1, "layer.sep8", "", None, SEP),
    (1, "layer.hide", "menu.layer.hide", None, ITEM),
    (2, "layer.hide.hide", "menu.layer.hide.hide", None, ITEM),
    (2, "layer.hide.others", "menu.layer.hide.others", Some(dtool_core::commands::arrange::NODE_HIDE_OTHERS), ITEM),
    (1, "layer.show", "menu.layer.show", None, ITEM),
    (2, "layer.show.show", "menu.layer.show.show", None, ITEM),
    (2, "layer.show.all", "menu.layer.show.all", None, ITEM),
    (2, "layer.show.others", "menu.layer.show.others", Some(dtool_core::commands::arrange::NODE_SHOW_OTHERS), ITEM),
    (1, "layer.sep9", "", None, SEP),
    (1, "layer.find_in_layers_panel", "menu.layer.find_in_layers_panel", None, ITEM),
    (1, COLLAPSE_LAYERS_MENU_ID, "menu.layer.collapse_all_in_layers_panel", None, ITEM),
    (1, "layer.sep10", "", None, SEP),
    (1, "layer.resources", "menu.layer.resources", None, ITEM),
    (2, "layer.resources.passthrough_pdf", "menu.layer.resources.passthrough_pdf", None, TOG),
    (2, "layer.resources.interpret_pdf", "menu.layer.resources.interpret_pdf", None, TOG),
    (1, "layer.add_caption", "menu.layer.add_caption", None, ITEM),
    (1, "layer.sep11", "", None, SEP),
    (1, "layer.float_with_text", "menu.layer.float_with_text", None, TOG),
    (1, "layer.inline_in_text", "menu.layer.inline_in_text", None, TOG),
    (1, "layer.sep12", "", None, SEP),
    (1, "layer.new_layer", "menu.layer.new_layer", None, ITEM),
    (2, "layer.new_layer.new_layer_from_snapshot", "menu.layer.new_layer.new_layer_from_snapshot", None, ITEM),
    (3, "layer.new_layer.new_layer_from_snapshot.none", "menu.layer.new_layer.new_layer_from_snapshot.none", None, ITEM),
    (1, "layer.sep13", "", None, SEP),
    (1, "layer.sep14", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep1", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep2", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep3", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep4", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep5", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep6", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep7", "", None, SEP),
    (2, "layer.serif_affinity_workspaces_workspacemenuitem.sep8", "", None, SEP),
    (0, "view", "menu.view", None, ITEM),
    (1, "view.zoom", "menu.view.zoom", None, ITEM),
    (2, "view.zoom.in", "menu.view.zoom.in", Some("view.zoom.set"), ITEM),
    (2, "view.zoom.out", "menu.view.zoom.out", Some("view.zoom.set"), ITEM),
    (2, "view.zoom.to_fit", "menu.view.zoom.to_fit", Some("view.zoom.fit"), ITEM),
    (2, "view.zoom.to_width", "menu.view.zoom.to_width", Some("view.zoom.to_width"), ITEM),
    (2, "view.zoom.to_selection", "menu.view.zoom.to_selection", Some("view.zoom.to_selection"), ITEM),
    (2, "view.zoom.to_document", "menu.view.zoom.to_document", Some("view.zoom.to_document"), ITEM),
    (2, "view.zoom.n100pct", "menu.view.zoom.n100pct", Some("view.zoom.actual"), ITEM),
    (2, "view.zoom.n200pct", "menu.view.zoom.n200pct", Some("view.zoom.set"), ITEM),
    (2, "view.zoom.n400pct", "menu.view.zoom.n400pct", Some("view.zoom.set"), ITEM),
    (2, "view.zoom.n800pct", "menu.view.zoom.n800pct", Some("view.zoom.set"), ITEM),
    (2, "view.zoom.actual_size", "menu.view.zoom.actual_size", Some("view.zoom.actual"), ITEM),
    (2, "view.zoom.pixel_size", "menu.view.zoom.pixel_size", None, ITEM),
    (1, "view.sep1", "", None, SEP),
    (1, "view.rotate", "menu.view.rotate", None, ITEM),
    (2, "view.rotate.left", "menu.view.rotate.left", Some("view.rotate.set"), ITEM),
    (2, "view.rotate.right", "menu.view.rotate.right", Some("view.rotate.set"), ITEM),
    (2, "view.rotate.sep1", "", None, SEP),
    (2, "view.rotate.reset", "menu.view.rotate.reset", Some("view.rotate.reset"), ITEM),
    (1, "view.mode", "menu.view.mode", None, ITEM),
    (2, "view.mode.pixels", "menu.view.mode.pixels", Some("view.flags.set"), TOG),
    (2, "view.mode.pixels_retina", "menu.view.mode.pixels_retina", Some("view.flags.set"), TOG),
    (2, "view.mode.vector", "menu.view.mode.vector", Some("view.flags.set"), TOG),
    (2, "view.mode.wireframe", "menu.view.mode.wireframe", None, ITEM),
    (3, "view.mode.wireframe.last_used", "menu.view.mode.wireframe.last_used", Some("view.flags.set"), ITEM),
    (3, "view.mode.wireframe.sep1", "", None, SEP),
    (3, "view.mode.wireframe.x_ray", "menu.view.mode.wireframe.x_ray", Some("view.xray"), TOG),
    (3, "view.mode.wireframe.outline", "menu.view.mode.wireframe.outline", Some("view.flags.set"), TOG),
    (2, "view.mode.hairline", "menu.view.mode.hairline", None, TOG),
    (2, "view.mode.sep1", "", None, SEP),
    (2, "view.mode.greyscale", "menu.view.mode.greyscale", None, TOG),
    (2, "view.mode.hide_effects", "menu.view.mode.hide_effects", None, TOG),
    (2, "view.mode.sep2", "", None, SEP),
    (2, "view.mode.single_view", "menu.view.mode.single_view", None, TOG),
    (2, "view.mode.split_view", "menu.view.mode.split_view", None, TOG),
    (1, "view.sep2", "", None, SEP),
    (1, "view.show", "menu.view.show", None, ITEM),
    (2, "view.show.margins", "menu.view.show.margins", Some("view.flags.set"), TOG),
    (2, "view.show.bleed", "menu.view.show.bleed", Some("view.flags.set"), TOG),
    (2, "view.show.guides", "menu.view.show.guides", Some("guides.show"), TOG),
    (2, "view.show.column_guides", "menu.view.show.column_guides", Some("guides.show"), TOG),
    (2, "view.show.text_flow", "menu.view.show.text_flow", None, TOG),
    (2, "view.show.word_count", "menu.view.show.word_count", None, TOG),
    (2, "view.show.sep1", "", None, SEP),
    (2, "view.show.grid", "menu.view.show.grid", Some("grid.set"), TOG),
    (2, "view.show.baseline_grid", "menu.view.show.baseline_grid", Some("grid.set"), TOG),
    (2, "view.show.pixel_grid", "menu.view.show.pixel_grid", Some("grid.set"), TOG),
    (2, "view.show.sep2", "", None, SEP),
    (2, "view.show.pixel_selection", "menu.view.show.pixel_selection", Some("view.flags.set"), TOG),
    (2, "view.show.sep3", "", None, SEP),
    (2, "view.show.rulers", "menu.view.show.rulers", Some("ruler.show"), TOG),
    (2, "view.show.text_ruler", "menu.view.show.text_ruler", Some("ruler.show"), TOG),
    (2, "view.show.sep4", "", None, SEP),
    (2, "view.show.slices", "menu.view.show.slices", None, TOG),
    (2, "view.show.page_edges_in_preview", "menu.view.show.page_edges_in_preview", Some("view.flags.set"), TOG),
    (1, "view.preview_mode", "menu.view.preview_mode", Some("view.flags.set"), TOG),
    (1, "view.sep3", "", None, SEP),
    (1, "view.clip_to_canvas", "menu.view.clip_to_canvas", Some("view.flags.set"), TOG),
    (1, "view.lock_guides", "menu.view.lock_guides", Some("guides.show"), TOG),
    (1, "view.sep4", "", None, SEP),
    (1, "view.snapping", "menu.view.snapping", None, ITEM),
    (1, "view.margins_and_guides", "menu.view.margins_and_guides", None, ITEM),
    (1, "view.grid_and_axis", "menu.view.grid_and_axis", None, ITEM),
    (1, "view.baseline_grid", "menu.view.baseline_grid", None, ITEM),
    (1, "view.sep5", "", None, SEP),
    (1, "view.new_view", "menu.view.new_view", None, ITEM),
    (1, "view.views", "menu.view.views", None, ITEM),
    (1, "view.sep6", "", None, SEP),
    (1, "view.move_to_previous_view_point", "menu.view.move_to_previous_view_point", None, ITEM),
    (1, "view.move_to_next_view_point", "menu.view.move_to_next_view_point", None, ITEM),
    (1, "view.sep7", "", None, SEP),
    (1, "view.top_toolbar", "menu.view.top_toolbar", None, ITEM),
    (2, "view.top_toolbar.show", "menu.view.top_toolbar.show", None, TOG),
    (2, crate::shell::CUSTOMISE_TOOLBAR_MENU_ID, "menu.view.top_toolbar.customise", None, ITEM),
    (1, "view.context_toolbar", "menu.view.context_toolbar", None, ITEM),
    (2, "view.context_toolbar.dock", "menu.view.context_toolbar.dock", None, TOG),
    (2, "view.context_toolbar.show", "menu.view.context_toolbar.show", None, TOG),
    (1, "view.tools", "menu.view.tools", None, ITEM),
    (2, "view.tools.dock", "menu.view.tools.dock", None, TOG),
    (2, "view.tools.show", "menu.view.tools.show", None, TOG),
    (2, "view.tools.sub_tools", "menu.view.tools.sub_tools", None, TOG),
    (2, crate::shell::CUSTOMISE_TOOLS_MENU_ID, "menu.view.tools.customise", None, ITEM),
    (1, "view.sep8", "", None, SEP),
    (1, "view.toggle_ui", "menu.view.toggle_ui", None, ITEM),
    (0, "window", "menu.window", None, ITEM),
    (1, "window.arrange", "menu.window.arrange", None, ITEM),
    (2, "window.arrange.float", "menu.window.arrange.float", None, ITEM),
    (2, "window.arrange.float_all", "menu.window.arrange.float_all", None, ITEM),
    (2, "window.arrange.dock", "menu.window.arrange.dock", None, ITEM),
    (2, "window.arrange.dock_all", "menu.window.arrange.dock_all", None, ITEM),
    (1, "window.sep1", "", None, SEP),
    (1, "window.panels", "menu.window.panels", None, ITEM),
    (2, "window.panels.add_preset", "menu.window.panels.add_preset", None, ITEM),
    (2, "window.panels.manage_presets", "menu.window.panels.manage_presets", None, ITEM),
    (2, "window.panels.reference", "menu.window.panels.reference", None, ITEM),
    (3, "window.panels.reference.vector", "workspace.vector", None, ITEM),
    (3, "window.panels.reference.pixel", "workspace.pixel", None, ITEM),
    (3, "window.panels.reference.layout", "workspace.layout", None, ITEM),
    (3, "window.panels.reference.previous", "menu.window.panels.reference.previous", None, ITEM),
    (2, "window.panels.sep1", "", None, SEP),
    (2, "window.panels.show_left", "menu.window.panels.show_left", None, TOG),
    (2, "window.panels.show_right", "menu.window.panels.show_right", None, TOG),
    (2, "window.panels.show_top", "menu.window.panels.show_top", None, TOG),
    (2, "window.panels.show_bottom", "menu.window.panels.show_bottom", None, TOG),
    (2, "window.panels.sep2", "", None, SEP),
    (2, "window.panels.hide", "menu.window.panels.hide", None, ITEM),
    (2, "window.panels.reset", "menu.window.panels.reset", None, ITEM),
    (1, "window.sep2", "", None, SEP),
    (1, "window.general", "menu.window.general", None, ITEM),
    (2, "window.general.assets", "menu.window.general.assets", None, TOG),
    (2, "window.general.brushes", "menu.window.general.brushes", None, TOG),
    (2, "window.general.colour", "menu.window.general.colour", None, TOG),
    (2, "window.general.export_layers", "menu.window.general.export_layers", None, TOG),
    (2, "window.general.export_options", "menu.window.general.export_options", None, TOG),
    (2, "window.general.history", "menu.window.general.history", None, TOG),
    (2, "window.general.layers", "menu.window.general.layers", None, TOG),
    (2, "window.general.links", "menu.window.general.links", None, TOG),
    (2, "window.general.navigator", "menu.window.general.navigator", None, TOG),
    (2, "window.general.quick_adjustments", "menu.window.general.quick_adjustments", None, TOG),
    (2, "window.general.quick_export", "menu.window.general.quick_export", None, TOG),
    (2, "window.general.slices", "menu.window.general.slices", None, TOG),
    (2, "window.general.snapshots", "menu.window.general.snapshots", None, TOG),
    (2, "window.general.states", "menu.window.general.states", None, TOG),
    (2, "window.general.swatches", "menu.window.general.swatches", None, TOG),
    (2, "window.general.transform", "menu.window.general.transform", None, TOG),
    (1, "window.help", "menu.window.help", None, ITEM),
    (2, "window.help.ask", "menu.window.help.ask", None, TOG),
    (2, "window.help.help", "menu.window.help.help", None, TOG),
    (1, "window.layout", "menu.window.layout", None, ITEM),
    (2, "window.layout.books", "menu.window.layout.books", None, TOG),
    (2, "window.layout.data_merge", "menu.window.layout.data_merge", None, TOG),
    (2, "window.layout.epub", "menu.window.layout.epub", None, TOG),
    (2, "window.layout.pages", "menu.window.layout.pages", None, TOG),
    (2, "window.layout.preflight", "menu.window.layout.preflight", None, TOG),
    (2, "window.layout.reading_order", "menu.window.layout.reading_order", None, TOG),
    (2, "window.layout.stock", "menu.window.layout.stock", None, TOG),
    (2, "window.layout.tags", "menu.window.layout.tags", None, TOG),
    (1, "window.pixel", "menu.window.pixel", None, ITEM),
    (2, "window.pixel.n32_bit_preview", "menu.window.pixel.n32_bit_preview", None, TOG),
    (2, "window.pixel.adjustments", "menu.window.pixel.adjustments", None, TOG),
    (2, "window.pixel.batch", "menu.window.pixel.batch", None, TOG),
    (2, "window.pixel.brushes", "menu.window.pixel.brushes", None, TOG),
    (2, "window.pixel.channels", "menu.window.pixel.channels", None, TOG),
    (2, "window.pixel.detections", "menu.window.pixel.detections", None, TOG),
    (2, "window.pixel.histogram", "menu.window.pixel.histogram", None, TOG),
    (2, "window.pixel.info", "menu.window.pixel.info", None, TOG),
    (2, "window.pixel.library", "menu.window.pixel.library", None, TOG),
    (2, "window.pixel.macro", "menu.window.pixel.macro", None, TOG),
    (2, "window.pixel.metadata", "menu.window.pixel.metadata", None, TOG),
    (2, "window.pixel.plate_solving", "menu.window.pixel.plate_solving", None, TOG),
    (2, "window.pixel.scope", "menu.window.pixel.scope", None, TOG),
    (2, "window.pixel.sources", "menu.window.pixel.sources", None, TOG),
    (1, "window.references", "menu.window.references", None, ITEM),
    (2, "window.references.anchors", "menu.window.references.anchors", None, TOG),
    (2, "window.references.cross_references", "menu.window.references.cross_references", None, TOG),
    (2, "window.references.fields", "menu.window.references.fields", None, TOG),
    (2, "window.references.hyperlinks", "menu.window.references.hyperlinks", None, TOG),
    (2, "window.references.index", "menu.window.references.index", None, TOG),
    (2, "window.references.notes", "menu.window.references.notes", None, TOG),
    (2, "window.references.table_of_contents", "menu.window.references.table_of_contents", None, TOG),
    (1, "window.scripting", "menu.window.scripting", None, ITEM),
    (2, "window.scripting.log_console", "menu.window.scripting.log_console", None, TOG),
    (2, "window.scripting.running_scripts", "menu.window.scripting.running_scripts", None, TOG),
    (2, "window.scripting.script_editor", "menu.window.scripting.script_editor", None, TOG),
    (2, "window.scripting.scripts_library", "menu.window.scripting.scripts_library", None, TOG),
    (1, "window.table", "menu.window.table", None, ITEM),
    (2, "window.table.table", "menu.window.table.table", None, TOG),
    (2, "window.table.table_formats", "menu.window.table.table_formats", None, TOG),
    (1, "window.text", "menu.window.text", None, ITEM),
    (2, "window.text.character", "menu.window.text.character", None, TOG),
    (2, "window.text.find_and_replace", "menu.window.text.find_and_replace", None, TOG),
    (2, "window.text.glyph_browser", "menu.window.text.glyph_browser", None, TOG),
    (2, "window.text.paragraph", "menu.window.text.paragraph", None, TOG),
    (2, "window.text.pinning", "menu.window.text.pinning", None, TOG),
    (2, "window.text.text_frame", "menu.window.text.text_frame", None, TOG),
    (2, "window.text.text_styles", "menu.window.text.text_styles", None, TOG),
    (2, "window.text.typography", "menu.window.text.typography", None, TOG),
    (1, "window.vector", "menu.window.vector", None, ITEM),
    (2, "window.vector.appearance", "menu.window.vector.appearance", None, TOG),
    (2, "window.vector.blend", "menu.window.vector.blend", None, TOG),
    (2, "window.vector.constraints", "menu.window.vector.constraints", None, TOG),
    (2, "window.vector.isometric", "menu.window.vector.isometric", None, TOG),
    (2, "window.vector.path_brushes", "menu.window.vector.path_brushes", None, TOG),
    (2, "window.vector.quick_fx", "menu.window.vector.quick_fx", None, TOG),
    (2, "window.vector.stroke", "menu.window.vector.stroke", None, TOG),
    (2, "window.vector.styles", "menu.window.vector.styles", None, TOG),
    (2, "window.vector.symbols", "menu.window.vector.symbols", None, TOG),
    (0, "help", "menu.help", None, ITEM),
    (1, "help.view_help", "menu.help.view_help", None, ITEM),
    (1, "help.feature_finder", "menu.help.feature_finder", None, ITEM),
    (1, "help.affinity_learning_portal", "menu.help.affinity_learning_portal", None, ITEM),
    (1, "help.send_feedback", "menu.help.send_feedback", None, ITEM),
    (1, "help.sep1", "", None, SEP),
    (1, "help.benchmark", "menu.help.benchmark", None, ITEM),
    (1, "help.sdk_documentation", "menu.help.sdk_documentation", None, ITEM),
    (1, "help.sep2", "", None, SEP),
    (1, "help.affinity_terms", "menu.help.affinity_terms", None, ITEM),
    (1, "help.privacy_policy", "menu.help.privacy_policy", None, ITEM),
    (1, "help.about", "menu.help.about", Some("app.about"), ITEM),
];

fn build_level(rows: &[MenuRow], index: &mut usize, depth: u8) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = Vec::new();
    while *index < rows.len() {
        let (row_depth, id, key, command, kind) = rows[*index];
        if row_depth < depth {
            break;
        }
        if row_depth > depth {
            match items.last_mut() {
                Some(parent) => {
                    parent.children = build_level(rows, index, row_depth);
                }
                None => *index += 1,
            }
            continue;
        }
        *index += 1;
        let separator = kind == MenuRowKind::Separator;
        let text_insert = crate::menus::text_insert::menu_command(id);
        items.push(MenuItem {
            id: MenuId(id),
            key,
            command: command.or(text_insert),
            children: Vec::new(),
            separator,
            planned: false,
            checkable: text_insert.is_none() && matches!(kind, MenuRowKind::Toggle | MenuRowKind::Radio),
        });
    }
    items
}

fn radio_rows() -> &'static std::collections::HashSet<&'static str> {
    static ROWS: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| PIXEL_MENU_ROWS.iter().filter(|row| row.4 == MenuRowKind::Radio).map(|row| row.1).collect())
}

pub fn is_radio_row(id: &str) -> bool {
    radio_rows().contains(id)
}

fn mark_planned(items: &mut [MenuItem]) {
    for item in items.iter_mut() {
        mark_planned(&mut item.children);
        item.planned = !item.separator && item.command.is_none() && item.children.is_empty()
            && !host_document_dialog(item.id.0) && !host_app_dialog(item.id.0) && !host_layers_action(item.id.0);
    }
}

impl MenuItem {
    pub fn label(&self) -> String {
        if self.separator {
            String::new()
        } else {
            text(self.key).to_string()
        }
    }

    pub fn find(&self, id: &str) -> Option<&MenuItem> {
        if self.id.0 == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    pub fn to_json(&self) -> Value {
        let mut item = json!({
            "id": self.id.0,
            "key": self.key,
            "label": self.label(),
            "command": self.command,
            "separator": self.separator,
            "planned": self.planned,
            "checkable": self.checkable,
            "radio": self.checkable && is_radio_row(self.id.0),
            "checked": Value::Null,
            "children": self.children.iter().map(MenuItem::to_json).collect::<Vec<Value>>(),
        });
        if host_app_dialog(self.id.0) {
            item["host_action"] = Value::Bool(true);
            item["requires_document"] = Value::Bool(false);
        } else if host_document_dialog(self.id.0)
            || host_layers_action(self.id.0)
            || matches!(crate::longdoc_host::route(self.id.0), Some(crate::longdoc_host::MenuRoute::Panel(_)))
        {
            item["host_action"] = Value::Bool(true);
        }
        item
    }
}

fn host_app_dialog(id: &str) -> bool {
    matches!(id, crate::dialogs::NEW_DOCUMENT_MENU | crate::shell::CUSTOMISE_TOOLBAR_MENU_ID
        | crate::snapping_dialog::MENU_ID)
        || panels_action(id).is_some()
}

pub(crate) fn host_document_dialog(id: &str) -> bool {
    crate::grid_dialog::GridMenu::from_id(id).is_some()
        || crate::selection_modify_dialog::Kind::from_menu(id).is_some()
        || matches!(id, crate::shell::DOCUMENT_SETUP_MENU_ID | crate::shell::FONT_MANAGER_MENU_ID
            | crate::shell::RESOURCE_MANAGER_MENU_ID | crate::guides_dialog::SHOW_GUIDES_MANAGER
            | crate::guides_dialog::MARGINS_AND_GUIDES | BLEND_OPTIONS_MENU_ID | LAYER_EFFECTS_MENU_ID
            | crate::shell::CUSTOMISE_TOOLS_MENU_ID | crate::resize_canvas_dialog::MENU_ID
            | crate::selection_refine_dialog::MENU_ID)
}

impl MenuTree {
    pub fn for_persona(persona: &str) -> MenuTree {
        let rows = rows_for_persona(persona);
        let mut index = 0;
        let mut menus = build_level(rows, &mut index, 0);
        crate::selection_ui::add_to_curves_menu(&mut menus);
        mark_planned(&mut menus);
        MenuTree {
            persona: normalize_persona(persona).to_string(),
            menus,
        }
    }

    pub fn top_level_ids(&self) -> Vec<&'static str> {
        self.menus.iter().map(|item| item.id.0).collect()
    }

    pub fn find(&self, id: &str) -> Option<&MenuItem> {
        self.menus.iter().find_map(|item| item.find(id))
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.menus.iter().map(MenuItem::to_json).collect())
    }
}

pub fn normalize_persona(persona: &str) -> &'static str {
    let lower = persona.trim().to_ascii_lowercase();
    PERSONAS
        .iter()
        .copied()
        .find(|known| *known == lower)
        .unwrap_or("pixel")
}

fn rows_for_persona(_persona: &str) -> &'static [MenuRow] {
    PIXEL_MENU_ROWS
}

pub fn menu_tree_json(persona: &str) -> Value {
    let mut tree = MenuTree::for_persona(persona).to_json();
    annotate_bridge_rows(&mut tree);
    tree
}

pub fn bridge_command(action: &str) -> Option<(&'static str, Value)> {
    let command = match action {
        "group" => ("node.group", json!({})),
        "new_layer_folder" => ("node.folder.create", json!({ "name": dtool_strings::text("layers.folder") })),
        "new_pixel_layer" => (dtool_core::commands::layer_kinds::LAYER_PIXEL_ADD, json!({ "current_surface": true })),
        "new_fill_layer" => (dtool_core::commands::layer_kinds::LAYER_FILL_ADD, json!({ "current_surface": true })),
        "ungroup" => ("node.ungroup", json!({})),
        "ungroup_all" => ("node.ungroup", json!({ "all": true })),
        "lock" => ("node.lock_state", json!({ "state": "locked" })),
        "unlock" => ("node.lock_state", json!({ "state": "unlocked" })),
        "unlock_all" => ("node.unlock_all", json!({})),
        "hide" => ("node.update", json!({ "set": { "visible": false } })),
        "show" => ("node.update", json!({ "set": { "visible": true } })),
        "show_all" => ("node.show_all", json!({})),
        "arrange_to_front" => ("node.arrange", json!({ "step": "to_front" })),
        "arrange_forward" => ("node.arrange", json!({ "step": "forward" })),
        "arrange_backward" => ("node.arrange", json!({ "step": "backward" })),
        "arrange_to_back" => ("node.arrange", json!({ "step": "to_back" })),
        "flip_horizontal" => ("transform.flip", json!({ "axis": "Horizontal" })),
        "flip_vertical" => ("transform.flip", json!({ "axis": "Vertical" })),
        "rotate_left" => ("transform.rotate_90", json!({ "direction": "Left" })),
        "rotate_right" => ("transform.rotate_90", json!({ "direction": "Right" })),
        _ => return None,
    };
    Some(command)
}

pub fn annotate_bridge_rows(items: &mut Value) {
    if let Some(items) = items.as_array_mut() {
        for item in items {
            if let Some(children) = item.get_mut("children") { annotate_bridge_rows(children); }
            let Some(id) = item.get("id").and_then(Value::as_str) else { continue; };
            if let Some(MenuBinding::ViewMode(mode)) = menu_binding(id) {
                item["params"] = json!({ "view_mode": mode });
                continue;
            }
            let Some(action) = bridge_action(id) else { continue; };
            let Some((name, params)) = bridge_command(action) else { continue; };
            item["command"] = Value::String(name.to_string());
            item["params"] = params;
            item["needs_selection"] = Value::Bool(crate::ui_bridge::UiBridge::needs_selection(action));
        }
    }
}


fn register_menu_provider(provider: fn(&str) -> Value) -> bool {
    dtool_core::commands::menus::register_menu_provider(provider)
}

fn resolve_items(items: &mut Value, session: &dtool_core::Session) {
    dtool_core::commands::menus::resolve_items(items, session);
    resolve_to_curves(items, session);
}

fn resolve_to_curves(items: &mut Value, session: &dtool_core::Session) {
    if let Some(items) = items.as_array_mut() {
        for item in items {
            if let Some(children) = item.get_mut("children") { resolve_to_curves(children, session); }
            let candidate = item.get("command").and_then(Value::as_str).or_else(|| item.get("id").and_then(Value::as_str)).unwrap_or("");
            if candidate != "selection.toCurves" { continue; }
            item["command"] = Value::String("selection.toCurves".into());
            item["planned"] = Value::Bool(false);
            item["enabled"] = Value::Bool(session.can_execute_named("selection.toCurves"));
            item["planned_key"] = Value::Null;
            item["error_code"] = Value::Null;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuBinding {
    ZoomBy(f32),
    ZoomTo(f32),
    ZoomFit,
    ZoomFitWidth,
    ZoomFitSelection,
    ZoomFitDocument,
    ZoomActual,
    RotateBy(f32),
    RotateReset,
    RulerShow,
    TextRulerShow,
    SaveHistory,
    ViewMode(dtool_core::ViewMode),
    Xray,
    ClipToCanvas,
    PreviewMode,
    ShowBleed,
    ShowMargins,
    ShowPreviewPageEdges,
    ShowPixelSelection,
    QuickMask,
    ShowGuides,
    ShowColumnGuides,
    LockGuides,
    ShowGrid,
    ShowBaselineGrid,
    ShowPixelGrid,
    HighlightMissingFonts,
    HighlightMissingChars,
    SelectSameLayer(&'static str),
    FillRule(dtool_core::boolean::FillRule),
}

pub const ROTATE_STEP_DEG: f32 = 15.0;

pub fn menu_binding(id: &str) -> Option<MenuBinding> {
    Some(match id {
        "view.zoom.in" => MenuBinding::ZoomBy(2.0),
        "view.zoom.out" => MenuBinding::ZoomBy(0.5),
        "view.zoom.to_fit" => MenuBinding::ZoomFit,
        "view.zoom.to_width" => MenuBinding::ZoomFitWidth,
        "view.zoom.to_selection" => MenuBinding::ZoomFitSelection,
        "view.zoom.to_document" => MenuBinding::ZoomFitDocument,
        "view.zoom.n100pct" | "view.zoom.actual_size" => MenuBinding::ZoomActual,
        "view.zoom.n200pct" => MenuBinding::ZoomTo(2.0),
        "view.zoom.n400pct" => MenuBinding::ZoomTo(4.0),
        "view.zoom.n800pct" => MenuBinding::ZoomTo(8.0),
        "view.rotate.left" => MenuBinding::RotateBy(-ROTATE_STEP_DEG),
        "view.rotate.right" => MenuBinding::RotateBy(ROTATE_STEP_DEG),
        "view.rotate.reset" => MenuBinding::RotateReset,
        "view.show.rulers" => MenuBinding::RulerShow,
        "view.show.text_ruler" => MenuBinding::TextRulerShow,
        "file.save_history_with_document" => MenuBinding::SaveHistory,
        "view.clip_to_canvas" => MenuBinding::ClipToCanvas,
        "view.preview_mode" => MenuBinding::PreviewMode,
        "view.mode.pixels" => MenuBinding::ViewMode(dtool_core::ViewMode::Pixel),
        "view.mode.pixels_retina" => MenuBinding::ViewMode(dtool_core::ViewMode::Pixel2x),
        "view.mode.vector" => MenuBinding::ViewMode(dtool_core::ViewMode::Normal),
        "view.mode.wireframe.x_ray" => MenuBinding::Xray,
        "view.mode.wireframe.outline" => MenuBinding::ViewMode(dtool_core::ViewMode::Outline),
        "view.mode.wireframe.last_used" => MenuBinding::ViewMode(dtool_core::ViewMode::Wireframe),
        "view.show.guides" => MenuBinding::ShowGuides,
        "view.show.column_guides" => MenuBinding::ShowColumnGuides,
        "view.lock_guides" => MenuBinding::LockGuides,
        "view.show.grid" => MenuBinding::ShowGrid,
        "view.show.baseline_grid" => MenuBinding::ShowBaselineGrid,
        "view.show.pixel_grid" => MenuBinding::ShowPixelGrid,
        "view.show.bleed" => MenuBinding::ShowBleed,
        "view.show.margins" => MenuBinding::ShowMargins,
        "view.show.page_edges_in_preview" => MenuBinding::ShowPreviewPageEdges,
        "view.show.pixel_selection" => MenuBinding::ShowPixelSelection,
        "pixel.pixel_selection.edit_as_layer" => MenuBinding::QuickMask,
        "text.highlights.highlight_missing_fonts" => MenuBinding::HighlightMissingFonts,
        "text.highlights.highlight_missing_characters" => MenuBinding::HighlightMissingChars,
        "layer.select.same.name" => MenuBinding::SelectSameLayer("Name"),
        "layer.select.same.tag_colour" => MenuBinding::SelectSameLayer("TagColour"),
        "vector.fill_mode.alternate_even_odd" => MenuBinding::FillRule(dtool_core::boolean::FillRule::EvenOdd),
        "vector.fill_mode.winding_non_zero" => MenuBinding::FillRule(dtool_core::boolean::FillRule::NonZero),
        _ => return None,
    })
}

impl MenuBinding {
    pub fn command_name(self) -> &'static str {
        match self {
            MenuBinding::ZoomBy(_) | MenuBinding::ZoomTo(_) => "view.zoom.set",
            MenuBinding::ZoomFit => "view.zoom.fit",
            MenuBinding::ZoomFitWidth => dtool_core::view::VIEW_ZOOM_TO_WIDTH,
            MenuBinding::ZoomFitSelection => dtool_core::view::VIEW_ZOOM_TO_SELECTION,
            MenuBinding::ZoomFitDocument => dtool_core::view::VIEW_ZOOM_TO_DOCUMENT,
            MenuBinding::ZoomActual => "view.zoom.actual",
            MenuBinding::RotateBy(_) => "view.rotate.set",
            MenuBinding::RotateReset => "view.rotate.reset",
            MenuBinding::RulerShow | MenuBinding::TextRulerShow => "ruler.show",
            MenuBinding::SaveHistory => "history.save_with_doc.set",
            MenuBinding::SelectSameLayer(_) => dtool_core::commands::selection_edit::EDIT_SELECT_SAME,
            MenuBinding::FillRule(_) => dtool_core::commands::path_nodes::PATH_FILL_RULE,
            MenuBinding::QuickMask => dtool_core::commands::quickmask::TOGGLE,
            MenuBinding::Xray => dtool_core::commands::view_flags::VIEW_XRAY,
            MenuBinding::ViewMode(_) | MenuBinding::ClipToCanvas | MenuBinding::PreviewMode | MenuBinding::ShowBleed | MenuBinding::ShowMargins | MenuBinding::ShowPreviewPageEdges | MenuBinding::ShowPixelSelection | MenuBinding::HighlightMissingFonts | MenuBinding::HighlightMissingChars => "view.flags.set",
            MenuBinding::ShowGuides | MenuBinding::ShowColumnGuides | MenuBinding::LockGuides => "guides.show",
            MenuBinding::ShowGrid | MenuBinding::ShowBaselineGrid | MenuBinding::ShowPixelGrid => "grid.set",
        }
    }

    pub fn checked(self, session: &dtool_core::Session) -> Option<bool> {
        match self {
            MenuBinding::RulerShow => Some(session.document.rulers.show),
            MenuBinding::TextRulerShow => Some(session.document.rulers.show_text_ruler),
            MenuBinding::SaveHistory => Some(session.history.save_with_document),
            MenuBinding::ViewMode(mode) => Some(session.view.canvas_flags.view_mode == mode),
            MenuBinding::FillRule(rule) => Some(dtool_core::commands::path_nodes::fill_rule_checked(session, rule)),
            MenuBinding::Xray => Some(session.view.xray),
            MenuBinding::ClipToCanvas => Some(session.view.canvas_flags.clip_to_canvas),
            MenuBinding::PreviewMode => Some(session.view.canvas_flags.preview_mode),
            MenuBinding::ShowBleed => Some(session.view.canvas_flags.show_bleed),
            MenuBinding::ShowMargins => Some(session.view.canvas_flags.show_margins),
            MenuBinding::ShowPreviewPageEdges => Some(session.view.canvas_flags.show_page_edges_in_preview),
            MenuBinding::ShowPixelSelection => Some(session.view.canvas_flags.show_pixel_selection),
            MenuBinding::ShowGuides => Some(session.guides.show),
            MenuBinding::QuickMask => Some(session.quick_mask.active),
            MenuBinding::ShowColumnGuides => Some(session.guides.show_columns),
            MenuBinding::LockGuides => Some(session.guides.locked),
            MenuBinding::ShowGrid => Some(session.document.grid.show),
            MenuBinding::ShowBaselineGrid => Some(session.document.grid.show_baseline),
            MenuBinding::ShowPixelGrid => Some(session.document.grid.show_pixel),
            MenuBinding::HighlightMissingFonts => Some(session.view.highlight().0),
            MenuBinding::HighlightMissingChars => Some(session.view.highlight().1),
            _ => None,
        }
    }

    pub fn command(self, session: &dtool_core::Session, view_size: [f32; 2]) -> dtool_core::Command {
        let centre = [view_size[0] * 0.5, view_size[1] * 0.5];
        let params = match self {
            MenuBinding::ZoomBy(factor) => json!({"zoom": session.zoom_limits().clamp(session.view.zoom * factor), "around": centre, "view_size": view_size}),
            MenuBinding::ZoomTo(zoom) => json!({"zoom": zoom * session.actual_zoom(), "around": centre, "view_size": view_size}),
            MenuBinding::ZoomFit | MenuBinding::ZoomFitWidth | MenuBinding::ZoomFitSelection | MenuBinding::ZoomFitDocument => json!({"view_size": view_size}),
            MenuBinding::ZoomActual => json!({"around": centre, "view_size": view_size}),
            MenuBinding::RotateBy(step) => json!({"degrees": session.view.rotation_deg + step}),
            MenuBinding::RotateReset => json!({}),
            MenuBinding::RulerShow => json!({"show": !session.document.rulers.show}),
            MenuBinding::TextRulerShow => json!({"show_text_ruler": !session.document.rulers.show_text_ruler}),
            MenuBinding::SaveHistory => json!({"enabled": !session.history.save_with_document}),
            MenuBinding::ViewMode(mode) => json!({"view_mode": mode}),
            MenuBinding::FillRule(rule) => json!({"rule": rule}),
            MenuBinding::Xray => json!({}),
            MenuBinding::ClipToCanvas => json!({"clip_to_canvas": !session.view.canvas_flags.clip_to_canvas}),
            MenuBinding::PreviewMode => json!({"preview_mode": !session.view.canvas_flags.preview_mode}),
            MenuBinding::ShowBleed => json!({"show_bleed": !session.view.canvas_flags.show_bleed}),
            MenuBinding::ShowMargins => json!({"show_margins": !session.view.canvas_flags.show_margins}),
            MenuBinding::ShowPreviewPageEdges => json!({"show_page_edges_in_preview": !session.view.canvas_flags.show_page_edges_in_preview}),
            MenuBinding::ShowPixelSelection => json!({"show_pixel_selection": !session.view.canvas_flags.show_pixel_selection}),
            MenuBinding::QuickMask => json!({"on": !session.quick_mask.active}),
            MenuBinding::SelectSameLayer(attribute) => json!({"attribute": attribute, "layer_panel": true}),
            MenuBinding::ShowGuides => json!({"show": !session.guides.show}),
            MenuBinding::ShowColumnGuides => json!({"show_columns": !session.guides.show_columns}),
            MenuBinding::LockGuides => json!({"locked": !session.guides.locked}),
            MenuBinding::ShowGrid => json!({"show": !session.document.grid.show}),
            MenuBinding::ShowBaselineGrid => json!({"show_baseline": !session.document.grid.show_baseline}),
            MenuBinding::ShowPixelGrid => json!({"show_pixel": !session.document.grid.show_pixel}),
            MenuBinding::HighlightMissingFonts => json!({"highlight_missing_fonts": !session.view.highlight().0}),
            MenuBinding::HighlightMissingChars => json!({"highlight_missing_chars": !session.view.highlight().1}),
        };
        dtool_core::Command::with_params(self.command_name(), params)
    }
}

pub fn apply_bindings(items: &mut Value, session: &dtool_core::Session) {
    resolve_to_curves(items, session);
    apply_binding_rows(items, session);
}

pub const BRIDGE_MENU_ACTIONS: [(&str, &str); 21] = [
    ("layer.group", "group"),
    ("layer.new_folder", "new_layer_folder"),
    ("vector.new_container_layer", "new_layer_folder"),
    ("pixel.new_layer", "new_pixel_layer"),
    ("pixel.new_fill_layer", "new_fill_layer"),
    ("layer.ungroup", "ungroup"),
    ("layer.ungroup_all", "ungroup_all"),
    ("layer.lock", "lock"),
    ("layer.unlock", "unlock"),
    ("layer.unlock_all", "unlock_all"),
    ("layer.hide.hide", "hide"),
    ("layer.show.show", "show"),
    ("layer.show.all", "show_all"),
    ("layer.arrange.move.to_front", "arrange_to_front"),
    ("layer.arrange.move.forward_one", "arrange_forward"),
    ("layer.arrange.move.back_one", "arrange_backward"),
    ("layer.arrange.move.to_back", "arrange_to_back"),
    ("layer.arrange.flip_horizontal", "flip_horizontal"),
    ("layer.arrange.flip_vertical", "flip_vertical"),
    ("layer.arrange.rotate_left", "rotate_left"),
    ("layer.arrange.rotate_right", "rotate_right"),
];

pub fn bridge_action(id: &str) -> Option<&'static str> {
    BRIDGE_MENU_ACTIONS.iter().find(|(menu, _)| *menu == id).map(|(_, action)| *action)
}

pub fn bridge_action_enabled(action: &str, session: &dtool_core::Session) -> bool {
    if !session.has_document { return false; }
    if matches!(action, "new_pixel_layer" | "new_fill_layer") {
        return bridge_command(action).is_some_and(|(name, params)| session.can_execute(&dtool_protocol::Command::with_params(name, params)));
    }
    if let Some(lock_action) = crate::menus::layer_lock::for_bridge(action) {
        return crate::menus::layer_lock::is_enabled(session, lock_action);
    }
    !crate::ui_bridge::UiBridge::needs_selection(action) || !crate::dialogs::locked_delete::LockedDeleteDialog::selected_nodes(session).is_empty()
}

fn apply_binding_rows(items: &mut Value, session: &dtool_core::Session) {
    if let Some(items) = items.as_array_mut() {
        for item in items {
            if item["id"] == "document.pages.view_masters" {
                item["checked"] = json!(crate::menus::pages::viewed_master(session).is_some());
            }
            if item["id"] == "pixel.pixel_selection.load_channel" {
                let children: Vec<_> = session.document.spare_channels.iter().enumerate().map(|(index, (name, mask))| {
                    json!({
                        "id": format!("{SELECTION_CHANNEL_PREFIX}{index}"),
                        "label": name,
                        "command": "selection.load",
                        "planned": false,
                        "enabled": dtool_core::selection::selection_size_matches_document(&session.document, mask)
                    })
                }).collect();
                item["enabled"] = Value::Bool(!children.is_empty());
                item["planned"] = Value::Bool(false);
                item["children"] = Value::Array(children);
                continue;
            }
            if let Some(children) = item.get_mut("children") { apply_binding_rows(children, session); }
            if let Some(command) = item["id"].as_str().and_then(|id| crate::menus::vector_warp::menu_command(session, id)) {
                item["command"] = Value::String(command.name.clone());
                item["planned"] = Value::Bool(false);
                item["enabled"] = Value::Bool(session.can_execute(&command));
                item["planned_key"] = Value::Null;
                item["error_code"] = Value::Null;
                continue;
            }
            if let Some(action) = item.get("id").and_then(Value::as_str).and_then(bridge_action) {
                item["planned"] = Value::Bool(false);
                item["enabled"] = Value::Bool(bridge_action_enabled(action, session));
                item["planned_key"] = Value::Null;
                item["error_code"] = Value::Null;
                continue;
            }
            let Some(binding) = item.get("id").and_then(Value::as_str).and_then(menu_binding) else { continue; };
            let enabled = session.has_document && session.can_execute(&binding.command(session, [800.0, 600.0]));
            item["command"] = Value::String(binding.command_name().into());
            item["planned"] = Value::Bool(false);
            item["enabled"] = Value::Bool(enabled);
            item["planned_key"] = Value::Null;
            item["error_code"] = Value::Null;
            item["checked"] = binding.checked(session).map_or(Value::Null, Value::Bool);
        }
    }
}

pub fn gui_row_gate(id: &str, session: &dtool_core::Session) -> Option<bool> {
    if id == BLEND_OPTIONS_MENU_ID {
        return Some(crate::dialogs::blend_options::can_edit_selection(session));
    }
    if id == LAYER_EFFECTS_MENU_ID {
        return Some(crate::mask_actions::target(session).is_some());
    }
    if id == FIND_LAYER_MENU_ID {
        return Some(crate::panels::layers::reveal_target(session).is_some());
    }
    if id == RENAME_LAYER_MENU_ID {
        return Some(crate::panels::layers::rename_target(session).is_some());
    }
    if let Some(action) = crate::menus::pages::Action::from_id(id) {
        return Some(action.enabled(session));
    }
    if let Some(command) = crate::menus::vector_warp::menu_command(session, id) {
        return Some(session.can_execute(&command));
    }
    if crate::menus::text_insert::handles(id) {
        return Some(session.text_edit.is_some());
    }
    if crate::table::menu::command_name(id).is_some() {
        return Some(crate::table::menu::command(session, id).is_some_and(|command| session.can_execute(&command)));
    }
    bridge_action(id).map(|action| bridge_action_enabled(action, session))
}

fn menu_row_state(id: &str, session: &dtool_core::Session) -> Option<(Value, Option<bool>)> {
    use dtool_core::commands::view_flags::{VIEW_FLAGS_SET, VIEW_XRAY};
    if let Some(command) = crate::table::menu::command(session, id) {
        return Some((command.params, None));
    }
    let binding = menu_binding(id)?;
    let size = match binding {
        MenuBinding::ZoomBy(_) | MenuBinding::ZoomTo(_) | MenuBinding::ZoomActual
        | MenuBinding::ZoomFit | MenuBinding::ZoomFitWidth | MenuBinding::ZoomFitSelection
        | MenuBinding::ZoomFitDocument => session.view_canvas_size?,
        MenuBinding::SelectSameLayer(_) => [0.0; 2],
        _ if matches!(binding.command_name(), VIEW_FLAGS_SET | VIEW_XRAY) => [0.0; 2],
        _ => return None,
    };
    Some((binding.command(session, size).params, binding.checked(session)))
}

pub fn install() -> bool {
    let gated = dtool_core::commands::menus::register_menu_row_gate(gui_row_gate);
    let state_registered = dtool_core::commands::menus::register_menu_row_state(menu_row_state);
    let keys = b11_ui_keys();
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    if dtool_strings::require(&refs).is_err() { return false; }
    register_menu_provider(menu_tree_json) && gated && state_registered
}

impl MenuItem {
    pub fn resolved_json(&self,session:&dtool_core::Session)->Value {
        let mut rows=Value::Array(vec![self.to_json()]);
        resolve_items(&mut rows,session);
        let mut item=rows.as_array_mut().unwrap().remove(0);
        localize_planned(&mut item);
        item
    }
}

fn localize_planned(item:&mut Value){
    if item.get("planned").and_then(Value::as_bool).unwrap_or(false){
        let label=item.get("label").and_then(Value::as_str).unwrap_or("");
        item["label"]=Value::String(format!("{} ({})",label,text("menu.planned.suffix")));
    }
    if let Some(children)=item.get_mut("children").and_then(Value::as_array_mut){for child in children{localize_planned(child);}}
}

impl MenuTree {
    pub fn resolved_json(&self,session:&dtool_core::Session)->Value {
        Value::Array(self.menus.iter().map(|item|item.resolved_json(session)).collect())
    }
}

#[derive(Debug,Clone)]
pub struct MenuAction {pub id:String,pub command:Option<String>}

fn app_action(id:&str)->bool{matches!(id,crate::home::HOME_MENU_ID|crate::dialogs::NEW_DOCUMENT_MENU|crate::dialogs::NEW_FROM_FAVOURITES_MENU|crate::dialogs::NEW_FROM_LAST_PRESET_MENU|crate::shell::EXIT_MENU_ID|"edit.settings"|"window.scripting.log_console"|"help.about"|"view.top_toolbar.customise"|"view.toggle_ui") || crate::settings::ToolbarArea::from_menu_id(id).is_some()}
pub fn shell_action(id:&str)->bool{app_action(id)||matches!(id,"edit.select_all"|"view.snapping"|"view.guides_manager"|crate::guides_dialog::MARGINS_AND_GUIDES|"help.feature_finder")}
pub const LAYER_EFFECTS_MENU_ID: &str = "layer.layer_effects";
pub const BLEND_OPTIONS_MENU_ID: &str = "layer.blend_options";
pub const COLLAPSE_LAYERS_MENU_ID: &str = "layer.collapse_all_in_layers_panel";
pub const FIND_LAYER_MENU_ID: &str = "layer.find_in_layers_panel";
pub const RENAME_LAYER_MENU_ID: &str = "layer.rename_layer";

pub(crate) fn host_layers_action(id: &str) -> bool {
    matches!(id, COLLAPSE_LAYERS_MENU_ID | FIND_LAYER_MENU_ID | RENAME_LAYER_MENU_ID)
}

pub fn panels_action(id:&str)->Option<&'static str>{["window.panels.reference.vector","window.panels.reference.pixel","window.panels.reference.layout","window.panels.reference.previous","window.panels.show_left","window.panels.show_right","window.panels.show_top","window.panels.show_bottom","window.panels.hide","window.panels.reset"].into_iter().find(|known|*known==id)}
pub fn preset_action(id:&str)->bool{matches!(id,"window.panels.add_preset"|"window.panels.manage_presets")}
pub const TEXT_STYLES_MENU_ROWS:&[&str]=&["text.text_styles.reapply_text_styles","text.text_styles.reapply_base_styles","text.text_styles.create_paragraph_style","text.text_styles.update_paragraph_style","text.text_styles.delete_paragraph_style","text.text_styles.create_character_style","text.text_styles.update_character_style","text.text_styles.delete_character_style"];

fn local_action(id:&str)->bool{(cfg!(windows)&&id=="file.print")||shell_action(id)||TEXT_STYLES_MENU_ROWS.contains(&id)||crate::batch_host::handles(id)||crate::float::handles(id)||crate::longdoc_host::handles(id)||id==crate::find_host::FIND_MENU_ID||id==crate::resize_canvas_dialog::MENU_ID||id==crate::resize_pixel_art_dialog::MENU_ID||crate::spell_host::handles(id)||crate::menus::text_insert::handles(id)||matches!(id,"file.open"|"file.place"|"file.save_as"|"file.save_as_package"|"file.export.as_template"|"file.export.export"|"file.import_content.from_file_s"|"document.resource_manager"|"view.grid_and_axis"|"view.baseline_grid"|"document.setup.document_setup"|"document.setup.assign_icc_profile"|"document.setup.resize.document"|"file.close"|"file.close_all"|"document.font_manager")||matches!(id,LAYER_EFFECTS_MENU_ID|BLEND_OPTIONS_MENU_ID|FIND_LAYER_MENU_ID|RENAME_LAYER_MENU_ID)}

pub const WINDOW_DOCUMENT_PREFIX: &str = "window.document.";
pub const SELECTION_CHANNEL_PREFIX: &str = "pixel.pixel_selection.load_channel.";

pub fn selection_channel_index(id: &str) -> Option<usize> {
    id.strip_prefix(SELECTION_CHANNEL_PREFIX)?.parse().ok()
}

pub fn window_document_index(id: &str) -> Option<usize> {
    id.strip_prefix(WINDOW_DOCUMENT_PREFIX)?.parse().ok()
}

pub fn show_queried_menu_bar(ui:&mut egui::Ui,session:Option<&mut dtool_core::Session>,persona:&str)->Option<MenuAction>{
    show_queried_menu_bar_with(ui,session,persona,&[])
}

pub fn show_queried_menu_bar_with(ui:&mut egui::Ui,session:Option<&mut dtool_core::Session>,persona:&str,documents:&[(String,bool)])->Option<MenuAction>{
    show_queried_menu_bar_recent(ui,session,persona,documents,&[])
}

pub fn queried_menu_tree(mut session:Option<&mut dtool_core::Session>,persona:&str,recent:&[std::path::PathBuf])->Value{
    let mut tree=match session.as_deref_mut() {
        Some(session)=>{let mut tree=session.run_named("ui.menu.query",json!({"persona":persona})).unwrap_or_else(|_|MenuTree::for_persona(persona).resolved_json(session));apply_bindings(&mut tree,session);tree},
        None=>MenuTree::for_persona(persona).to_json(),
    };
    crate::menus::text_format::annotate(&mut tree, session.as_deref());
    recent::inject_rows(&mut tree,recent);
    tree
}

pub fn show_queried_menu_bar_recent(ui:&mut egui::Ui,session:Option<&mut dtool_core::Session>,persona:&str,documents:&[(String,bool)],recent:&[std::path::PathBuf])->Option<MenuAction>{
    let clipboard_document = session.as_ref().is_some_and(|session| crate::os_clipboard::can_create_document(&session.clipboard));
    show_queried_menu_bar_with_toolbars(ui, session, persona, documents, recent, Default::default(), Default::default(), clipboard_document, false, None)
}

pub(crate) fn show_queried_menu_bar_with_toolbars(ui:&mut egui::Ui,mut session:Option<&mut dtool_core::Session>,persona:&str,documents:&[(String,bool)],recent:&[std::path::PathBuf],toolbars:crate::settings::ToolbarVisibility,dock:crate::dock::DockColumns,clipboard_document:bool,last_preset_available:bool,caption:Option<&str>)->Option<MenuAction>{
    let has_document=session.is_some();
    let mut tree=queried_menu_tree(session.as_deref_mut(),persona,recent);
    if let Some(menus) = tree.as_array_mut()
        && let Some(window) = menus.iter_mut().find(|menu| menu["id"] == "window")
        && let Some(children) = window["children"].as_array_mut()
        && let Some(panels) = children.iter_mut().find(|menu| menu["id"] == "window.panels")
        && let Some(rows) = panels["children"].as_array_mut()
    {
        for row in rows {
            if let Some(side) = row["id"].as_str()
                .and_then(|id| id.strip_prefix("window.panels.show_"))
                .and_then(crate::dock::DockSide::from_key)
            {
                row["checked"] = Value::Bool(dock.is_shown(side));
            }
        }
    }
    let mut action=None;
    let menu_item_height = if caption.is_some() { 29.0 } else { 23.0 };
    let menu_bar_height = if caption.is_some() { 36.0 } else { dtool_design::size("bar.menu.height") };
    egui::Panel::top("dtool.shell.menu_bar")
        .exact_size(menu_bar_height)
        .resizable(false)
        .frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "menu_bar")).inner_margin(egui::Margin::symmetric(8, 3)))
        .show(ui, |ui| {
    egui::MenuBar::new().style(move |style: &mut egui::Style| {
        egui::containers::menu::menu_style(style);
        command_menu_hover(style);
        style.spacing.item_spacing.x = 5.0;
        style.spacing.button_padding.x = 7.0;
        style.spacing.interact_size.y = menu_item_height;
    }).ui(ui,|ui|{
        show_menu_brand(ui);
        if let Some(menus)=tree.as_array(){for menu in menus{
            let label=menu.get("label").and_then(Value::as_str).unwrap_or("");
            let button = egui::Button::new(egui::RichText::new(label).size(dtool_design::size("menu.font_size")))
                .frame(true).frame_when_inactive(false);
            egui::containers::menu::MenuButton::from_button(button).ui(ui,|ui| {
                if menu["id"] == "file" && let Some(rows) = menu["children"].as_array() {
                    action = draw_file_menu(ui, rows, has_document, toolbars, clipboard_document, last_preset_available);
                    return;
                }
                let height = (ui.ctx().content_rect().height() - menu_bar_height - 16.0).max(1.0);
                scroll_command_menu(ui, height, |ui| {
                    let native_graphic = menu["id"] == "edit" && crate::os_clipboard::native_graphic_available();
                    if let Some(rows) = menu["children"].as_array()
                        && let Some(result) = draw_menu_rows(ui, rows, has_document, toolbars, clipboard_document, native_graphic, last_preset_available)
                    { action = Some(result); }
                    if menu.get("id").and_then(Value::as_str)==Some("view"){
                        let label = text("ruler.origin.title");
                        let submenu = ui.menu_button(label,|ui| context_command_menu(ui, |ui| {
                            if let Some(session)=session.as_deref_mut(){crate::rulers::origin_controls(ui,session);}
                        }));
                        command_menu_tooltip(ui, submenu.response, label, true, None);
                    }
                    if menu.get("id").and_then(Value::as_str)==Some("window")&&!documents.is_empty(){
                        ui.separator();
                        for (index,(label,active)) in documents.iter().enumerate(){
                            let mut checked=*active;
                            let response = dtool_design::menu_toggle(ui, &mut checked, label.as_str());
                            if command_menu_tooltip(ui, response, label, true, None).clicked(){action=Some(MenuAction{id:format!("{}{}",WINDOW_DOCUMENT_PREFIX,index),command:None});ui.close();}
                        }
                    }
                });
            });
        }}
        if let Some(title) = caption {
            draw_window_caption(ui, title);
        }
    });
        });
    action
}

fn draw_window_caption(ui: &mut egui::Ui, title: &str) {
    use egui::{Align, Button, Layout, Rect, Sense, Stroke, StrokeKind, ViewportCommand, vec2};

    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let size = vec2(35.0, 27.0);
        for (key, command) in [
            ("window.caption.close", ViewportCommand::Close),
            (if maximized { "window.caption.restore" } else { "window.caption.maximize" },
                ViewportCommand::Maximized(!maximized)),
            ("window.caption.minimize", ViewportCommand::Minimized(true)),
        ] {
            let label = text(key);
            let response = ui.add_sized(size, Button::new("").frame(false));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let center = response.rect.center();
            let stroke = Stroke::new(1.25, ui.style().interact(&response).fg_stroke.color);
            let painter = ui.painter();
            match &command {
                ViewportCommand::Close => {
                    painter.line_segment([center + vec2(-3.5, -3.5), center + vec2(3.5, 3.5)], stroke);
                    painter.line_segment([center + vec2(-3.5, 3.5), center + vec2(3.5, -3.5)], stroke);
                }
                ViewportCommand::Maximized(_) if maximized => {
                    let front = Rect::from_center_size(center + vec2(-1.0, 1.0), vec2(7.0, 7.0));
                    painter.rect_stroke(front, 0.0, stroke, StrokeKind::Middle);
                    painter.add(egui::Shape::line(vec![
                        center + vec2(-2.5, -2.5), center + vec2(-2.5, -4.5),
                        center + vec2(4.5, -4.5), center + vec2(4.5, 2.5),
                        center + vec2(2.5, 2.5),
                    ], stroke));
                }
                ViewportCommand::Maximized(_) => {
                    let rect = Rect::from_center_size(center, vec2(8.0, 8.0));
                    painter.rect_stroke(rect, 0.0, stroke, StrokeKind::Middle);
                }
                ViewportCommand::Minimized(_) => {
                    painter.line_segment([center + vec2(-4.0, 0.0), center + vec2(4.0, 0.0)], stroke);
                }
                _ => {}
            }
            if response.on_hover_text(label).clicked() {
                ui.ctx().send_viewport_cmd(command);
            }
        }

        let (rect, drag) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), size.y), Sense::click_and_drag());
        if drag.double_clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let text_rect = rect.shrink2(vec2(6.0, 0.0));
        if text_rect.is_positive() {
            let colour = ui.visuals().weak_text_color();
            let mut job = egui::text::LayoutJob::simple_singleline(title.to_owned(), egui::FontId::proportional(12.0), colour);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            let size = galley.size();
            let window_center = ui.ctx().content_rect().center().x;
            let left = (window_center - size.x * 0.5)
                .clamp(text_rect.left(), (text_rect.right() - size.x).max(text_rect.left()));
            if galley.elided {
                drag.on_hover_text(title);
            }
            ui.painter().with_clip_rect(text_rect).galley(egui::pos2(left, rect.center().y - size.y * 0.5), galley, colour);
        }
    });
}

pub fn row_is_local(id:&str,has_document:bool)->bool{
    if id == crate::shell::CUSTOMISE_TOOLS_MENU_ID {
        return has_document;
    }
    (local_action(id)&&(has_document||id=="file.open"||app_action(id)||crate::batch_host::runs_without_document(id)))||crate::panels::window_panel(id).is_some()||panels_action(id).is_some()||preset_action(id)
}

fn show_menu_brand(ui: &mut egui::Ui) {
    let size = egui::Vec2::splat(20.0);
    let key = egui::Id::new("dtool.menu_brand");
    let texture = ui.data_mut(|data| data.get_temp::<egui::TextureHandle>(key)).or_else(|| {
        let logo = dtool_design::icons::app_icon()?;
        let logo = dtool_export::raster::decode(logo, dtool_export::ExportFormat::Png).ok()?;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [logo.width() as usize, logo.height() as usize], logo.as_raw(),
        );
        let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
        let texture = ui.ctx().load_texture("DTool menu mark", image, options);
        ui.data_mut(|data| data.insert_temp(key, texture.clone()));
        Some(texture)
    });
    if let Some(texture) = texture {
        ui.add(egui::Image::new(&texture).fit_to_exact_size(size));
    }
}

fn draw_file_menu(ui: &mut egui::Ui, rows: &[Value], has_document: bool,
    toolbars: crate::settings::ToolbarVisibility, clipboard_document: bool,
    last_preset_available: bool) -> Option<MenuAction>
{
    configure_command_menu(ui);
    ui.set_width(680.0_f32.min((ui.ctx().content_rect().width() - 24.0).max(1.0)));
    let split = rows.iter().position(|row| row["id"] == "file.sep6").unwrap_or(rows.len());
    let native_graphic = crate::os_clipboard::native_graphic_available();
    let mut action = None;
    ui.columns(2, |columns| {
        let (left, right) = columns.split_at_mut(1);
        for (column, values) in [
            (&mut left[0], &rows[..split]),
            (&mut right[0], &rows[(split + 1).min(rows.len())..]),
        ] {
            if let Some(result) = draw_menu_rows(column, values, has_document,
                toolbars, clipboard_document, native_graphic, last_preset_available)
            {
                action = Some(result);
            }
        }
        let label = text("dialog.template.open");
        if command_menu_item(&mut right[0], label, true).clicked() {
            action = Some(MenuAction { id: "file.template.open".into(), command: None });
            right[0].close();
        }
    });
    action
}

fn command_menu_hover(style: &mut egui::Style) {
    style.visuals.widgets.hovered.weak_bg_fill = dtool_design::color("menu.row_hover");
    style.visuals.widgets.active.weak_bg_fill = dtool_design::color("menu.row_pressed");
    style.visuals.widgets.open.weak_bg_fill = dtool_design::color("menu.row_hover");
}

pub(crate) fn configure_command_menu(ui: &mut egui::Ui) {
    let available = ui.ctx().content_rect().width() - 16.0;
    ui.set_width(dtool_design::size("menu.width").min(available.max(1.0)));
    dtool_design::menu_row_style(ui.style_mut());
}

pub(crate) fn menu_choice(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    dtool_design::with_icon_selection(ui, |ui| {
        ui.add(egui::Button::new((label, egui::Atom::grow()))
            .selected(selected)
            .frame_when_inactive(selected)
            .min_size(egui::vec2(ui.available_width(), dtool_design::size("menu.row.height"))))
    })
}

pub(crate) fn command_menu_tooltip(
    ui: &egui::Ui,
    response: egui::Response,
    label: &str,
    has_icon: bool,
    path: Option<&str>,
) -> egui::Response {
    let spacing = ui.spacing();
    let icon_space = if has_icon { spacing.icon_width.max(dtool_design::size("control.switch.width")) + spacing.icon_spacing.max(6.0) } else { 0.0 };
    let text_width = response.rect.width() - 2.0 * spacing.button_padding.x - icon_space;
    let truncated = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(label.to_owned(), egui::FontId::proportional(dtool_design::size("menu.font_size")), egui::Color32::WHITE).size().x > text_width
    });
    let tooltip = match (truncated, path) {
        (true, Some(path)) => format!("{label}\n{path}"),
        (true, None) => label.to_owned(),
        (false, Some(path)) => path.to_owned(),
        (false, None) => return response,
    };
    let show = |ui: &mut egui::Ui| {
        ui.set_max_width(ui.spacing().tooltip_width);
        ui.add(egui::Label::new(tooltip).wrap());
    };
    if response.enabled() { response.on_hover_ui(show) }
    else { response.on_disabled_hover_ui(show) }
}

pub(crate) fn context_command_menu<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    canvas_command_menu(ui, false, content)
}

pub(crate) fn canvas_command_menu<R>(ui: &mut egui::Ui, two_columns: bool, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let viewport = ui.ctx().content_rect();
    let maximum_height = (viewport.height() - 24.0).max(1.0);
    if !two_columns {
        return scroll_command_menu(ui, maximum_height, content);
    }
    configure_command_menu(ui);
    ui.set_width(500.0_f32.min((viewport.width() - 16.0).max(1.0)));
    // A popup starts with an estimated height. Let the two columns size
    // the popup on a normal window so the bottom rows stay visible.
    if viewport.height() >= 560.0 && viewport.width() >= 380.0 {
        return content(ui);
    }
    egui::ScrollArea::vertical()
        .max_height(maximum_height)
        .auto_shrink([false, true])
        .show(ui, content)
        .inner
}

fn scroll_command_menu<R>(ui: &mut egui::Ui, maximum_height: f32, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    configure_command_menu(ui);
    egui::ScrollArea::vertical()
        .max_height(maximum_height)
        .auto_shrink([false, true])
        .show(ui, content)
        .inner
}

pub(crate) fn command_menu_item(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let width = ui.available_width();
    let response = ui.add_enabled(enabled, egui::Button::new((label, egui::Atom::grow()))
        .frame(true).frame_when_inactive(false)
        .min_size(egui::vec2(width, dtool_design::size("menu.row.height"))));
    command_menu_tooltip(ui, response, label, false, None)
}

fn draw_menu_rows(ui:&mut egui::Ui,rows:&[Value],has_document:bool,toolbars:crate::settings::ToolbarVisibility,clipboard_document:bool,native_graphic:bool,last_preset_available:bool)->Option<MenuAction>{
    let mut action=None;
    for item in rows {
        if item.get("separator").and_then(Value::as_bool).unwrap_or(false){ui.separator();continue;}
        let id=item.get("id").and_then(Value::as_str).unwrap_or("");
        let label=item.get("label").and_then(Value::as_str).unwrap_or("");
        if let Some(children) = item["children"].as_array().filter(|children| !children.is_empty()) {
            let button = egui::Button::new((label, egui::Atom::grow()))
                .right_text(egui::containers::menu::SubMenuButton::arrow_atom(None))
                .frame(true).frame_when_inactive(false)
                .min_size(egui::vec2(ui.available_width(), dtool_design::size("menu.row.height")));
            let (response, _) = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| context_command_menu(ui, |ui| {
                if let Some(result) = draw_menu_rows(ui,children,has_document,toolbars,clipboard_document,native_graphic,last_preset_available) {
                    action = Some(result);
                }
            }));
            command_menu_tooltip(ui, response, label, true, None);
        }else{
            let local=row_is_local(id,has_document);
            let clipboard_enabled = match id {
                "file.new_from_clipboard" => clipboard_document || native_graphic,
                "edit.paste.paste" => has_document && native_graphic,
                _ => false,
            };
            let locally_enabled = local && !matches!(id, FIND_LAYER_MENU_ID | RENAME_LAYER_MENU_ID | LAYER_EFFECTS_MENU_ID)
                && !crate::menus::text_insert::handles(id);
            let enabled = if id == crate::dialogs::NEW_FROM_LAST_PRESET_MENU {
                last_preset_available
            } else {
                clipboard_enabled || locally_enabled || item["enabled"].as_bool().unwrap_or(false)
            };
            let planned=item.get("planned").and_then(Value::as_bool).unwrap_or(true)&&!local;
            let label=if planned{format!("{} ({})",label,text("menu.planned.suffix"))}else{label.to_string()};
            let checked = crate::settings::ToolbarArea::from_menu_id(id).map(|area| area.checked(toolbars))
                .or_else(|| crate::panels::window_panel(id).map(|_| crate::panels::window_panel_checked(id)))
                .or_else(|| item.get("checked").and_then(Value::as_bool));
            let path = item.get("path").and_then(Value::as_str);
            let exclusive = item["radio"].as_bool().unwrap_or(false);
            let response = if let Some(mut checked) = checked {
                ui.add_enabled_ui(enabled, |ui| {
                    if exclusive { menu_choice(ui, &label, checked) }
                    else { dtool_design::menu_toggle(ui, &mut checked, label.as_str()) }
                }).inner
            } else {
                let mut shown = egui::RichText::new(label.as_str());
                if item["missing"].as_bool().unwrap_or(false) { shown = shown.weak(); }
                ui.add_enabled(enabled, egui::Button::new((shown, egui::Atom::grow()))
                    .frame(true).frame_when_inactive(false)
                    .min_size(egui::vec2(ui.available_width(), dtool_design::size("menu.row.height"))))
            };
            if command_menu_tooltip(ui, response, &label, checked.is_some() && !exclusive, path).clicked() {
                action=Some(MenuAction{id:id.into(),command:item.get("command").and_then(Value::as_str).map(str::to_string)});ui.close();
            }
        }
    }
    action
}

const B11_ADDITIONAL_KEYS: &[&str] = &[
    "caption.units.cm",
    "caption.units.in",
    "caption.units.mm",
    "caption.units.pt",
    "caption.units.px",
    "context.move.align",
    "dialog.template.open",
    "menu.context.add_to_swatches",
    "menu.context.convert_text_to_field",
    "menu.context.create_style",
    "menu.context.develop",
    "menu.context.insert_cross_reference",
    "menu.context.liquify",
    "menu.context.move_selection_to_new_layer",
    "menu.context.preflight_comment",
    "menu.planned.suffix",
    "ruler.origin.title",
    "status.hint.default",
];

pub fn b11_ui_keys() -> Vec<String> {
    fn collect(items: &[MenuItem], keys: &mut std::collections::BTreeSet<String>) {
        for item in items {
            if !item.separator && !item.key.is_empty() { keys.insert(item.key.to_string()); }
            collect(&item.children, keys);
        }
    }
    let mut keys = std::collections::BTreeSet::new();
    for persona in PERSONAS { collect(&MenuTree::for_persona(persona).menus, &mut keys); }
    for flyout in crate::tools_panel::default_flyouts() {
        for id in std::iter::once(flyout.parent).chain(flyout.children) {
            keys.insert(id.0.to_string());
            keys.insert(format!("tooltip.{}", id.0));
        }
    }
    keys.extend(B11_ADDITIONAL_KEYS.iter().map(|key| key.to_string()));
    keys.into_iter().collect()
}

pub fn missing_b11_keys() -> Vec<String> {
    let keys = b11_ui_keys();
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    dtool_strings::missing(&refs)
}
