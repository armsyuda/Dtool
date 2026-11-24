use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dtool_protocol::CommandError;

pub use dtool_protocol::CommandError as PrefsError;
use serde::{Deserialize, Serialize};

pub const CMD_QUERY: &str = "pref.query";
pub const CMD_SET: &str = "pref.set";
pub const CMD_PERFORMANCE_SET: &str = "pref.performance.set";
pub const DIALOG_GAP_SOURCE: &str = "DIALOG_GAP.md";
pub const DIALOG_GAP_FIELDS: [&str; 26] = [
    "reopen_docs",
    "limit_zoom_100",
    "open_floating",
    "auto_update_links",
    "save_thumbnails",
    "psd_save_over",
    "preserve_unicode_breaks",
    "smart_cut_paste",
    "filler_as_text",
    "psd_text_as_text",
    "disk_warn_gb",
    "dither_gradients",
    "precise_clipping",
    "recovery_interval_s",
    "retina_rendering",
    "opencl",
    "text_contrast",
    "tooltip_delay_ms",
    "decimals_mm",
    "lock_bg_on_import",
    "default_view_mode",
    "lines_in_points",
    "text_in_points",
    "ask_layer_name",
    "white_font_dropdown",
    "font_preview_text",
];
pub const EXTRA_GENERAL_FIELDS: [&str; 1] = ["open_in_active_group"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeItem {
    pub id: String,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextReplacement {
    pub replace: String,
    pub with: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuDevice {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub memory_mb: u32,
}

fn enumerate_gpu_devices() -> Vec<GpuDevice> {
    let mut devices = Vec::new();
    if let Ok(output) = std::process::Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "DeviceID,Name,AdapterRAM"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let id = parts[0].to_string();
                    let name = parts[1..parts.len() - 1].join(" ");
                    let memory_mb = parts.last()
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|v| (v / 1_048_576) as u32)
                        .unwrap_or(0);
                    let vendor = if name.contains("NVIDIA") {
                        "NVIDIA".to_string()
                    } else if name.contains("AMD") || name.contains("Radeon") {
                        "AMD".to_string()
                    } else if name.contains("Intel") {
                        "Intel".to_string()
                    } else {
                        String::new()
                    };
                    devices.push(GpuDevice {
                        id,
                        name,
                        vendor,
                        memory_mb,
                    });
                }
            }
        }
    }
    if devices.is_empty() {
        if let Ok(output) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object DeviceID,Name,AdapterRAM | ConvertTo-Csv -NoTypeInformation",
            ])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines().skip(1) {
                    let fields: Vec<&str> = line.split(',').collect();
                    if fields.len() >= 3 {
                        let id = fields[0].trim_matches('"').to_string();
                        let name = fields[1].trim_matches('"').to_string();
                        let memory_mb = fields[2]
                            .trim_matches('"')
                            .parse::<u64>()
                            .map(|v| (v / 1_048_576) as u32)
                            .unwrap_or(0);
                        let vendor = if name.contains("NVIDIA") {
                            "NVIDIA".to_string()
                        } else if name.contains("AMD") || name.contains("Radeon") {
                            "AMD".to_string()
                        } else if name.contains("Intel") {
                            "Intel".to_string()
                        } else {
                            String::new()
                        };
                        devices.push(GpuDevice {
                            id,
                            name,
                            vendor,
                            memory_mb,
                        });
                    }
                }
            }
        }
    }
    devices
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub reopen_docs: bool,
    pub limit_zoom_100: bool,
    pub open_floating: bool,
    pub open_in_active_group: bool,
    pub auto_update_links: bool,
    pub save_thumbnails: bool,
    pub psd_save_over: bool,
    pub preserve_unicode_breaks: bool,
    pub smart_cut_paste: bool,
    pub filler_as_text: bool,
    pub psd_text_as_text: bool,
    pub haptics: bool,
    pub copy_as_svg: bool,
    pub prefer_metafile_paste: bool,
    pub keep_selection_after_delete: bool,
    pub load_xmp_sidecars: bool,
    pub refine_heic_depth: bool,
    pub apply_gain_maps: bool,
    pub psd_smart_objects: bool,
    pub show_home_on_startup: bool,
    pub send_usage_data: bool,
    pub upload_crash_reports: bool,

    pub ml_models: Vec<String>,

    pub mcp_enabled: bool,
    pub mcp_desktop_files: bool,
    pub mcp_networks: bool,
    pub mcp_use_saved_scripts: bool,
    pub mcp_save_scripts: bool,
    pub mcp_local_hints: bool,
    pub mcp_share_hints: bool,

    pub scripting_enabled: bool,
    pub script_file_system: bool,
    pub script_networks: bool,
    pub script_dynamic_code: bool,
    pub script_folders: Vec<String>,

    pub rgb_profile: String,
    pub rgb32_profile: String,
    pub cmyk_profile: String,
    pub grey_profile: String,
    pub lab_profile: String,
    pub rendering_intent: String,
    pub black_point_compensation: bool,
    pub convert_to_working: bool,
    pub convert_warn: bool,
    pub warn_assign_unprofiled: bool,
    pub hdr_32bit_default: bool,
    pub ocio_config: String,
    pub ocio_by_filename: bool,
    pub ocio_warn_on_load: bool,
    pub exr_associate_alpha: bool,
    pub exr_post_divide: bool,
    pub exr_perturb_zero_alpha: bool,

    pub ram_limit: u32,
    pub disk_warn_gb: u32,
    pub undo_limit: u32,
    pub view_quality: String,
    pub dither_gradients: bool,
    pub precise_clipping: bool,
    pub recovery_interval_s: u32,
    pub renderer: String,
    pub retina_rendering: String,
    pub opencl: bool,
    pub gpu_device: String,

    pub artboard_grey: f32,
    pub text_contrast: f32,
    pub accent_colour: String,
    pub tooltip_delay_ms: u32,
    pub decimals_px: u32,
    pub decimals_pt: u32,
    pub decimals_pc: u32,
    pub decimals_deg: u32,
    pub decimals_in: u32,
    pub decimals_ft: u32,
    pub decimals_yd: u32,
    pub decimals_mi: u32,
    pub decimals_mm: u32,
    pub decimals_cm: u32,
    pub decimals_m: u32,
    pub decimals_km: u32,
    pub lock_bg_on_import: bool,
    pub default_view_mode: String,
    pub lines_in_points: bool,
    pub text_in_points: bool,
    pub show_brush_previews: bool,
    pub always_brush_crosshair: bool,
    pub auto_commit_filters: bool,
    pub pointer_support: bool,
    pub ask_layer_name: bool,
    pub white_font_dropdown: bool,
    pub font_preview_text: String,
    pub font_preview_samples: Vec<String>,

    pub bleed_colour: String,
    pub column_guide_fill_colour: String,
    pub margins_colour: String,
    pub column_guide_stroke_colour: String,
    pub guides_colour: String,
    pub baseline_grid_colour: String,
    pub snapping_grid_colour: String,
    pub snapping_grid_subdivision_colour: String,
    pub pixel_grid_colour: String,
    pub grid_link_defaults: bool,
    pub snapping_grid_opacity: f32,
    pub snapping_grid_subdivision_opacity: f32,
    pub pixel_grid_opacity: f32,

    pub home_items: Vec<HomeItem>,
    pub home_template_folders: Vec<PathBuf>,

    pub shift_cycles_tools: bool,
    pub wheel_zoom: bool,
    pub scrubby_zoom: bool,
    pub dial_support: bool,
    pub sync_tools: bool,
    pub touch_gestures_only: bool,
    pub alt_scroll_rotate: bool,
    pub marquee_behaviour: String,
    pub move_aspect_constrain: String,
    pub tablet_input: String,
    pub text_blend_gamma: f32,
    pub nudge_distance: f32,
    pub modifier_nudge_distance: f32,
    pub dictionary_folder: String,

    pub assistant_enabled: bool,
    pub assistant_auto_undo: bool,
    pub assistant_alert: bool,
    pub assist_paint_no_layer: String,
    pub assist_erase_vector: String,
    pub assist_brush_vector: String,
    pub assist_brush_sharing: String,
    pub assist_filter_non_pixel: String,
    pub assist_adjustment_selection: String,
    pub assist_mask_selection: String,
    pub assist_filter_layer_selection: String,
    pub assist_delete_image: String,
    pub assist_duplicate_image: String,

    pub plugin_folders: Vec<String>,
    pub allow_unknown_plugins: bool,

    pub linked_services: Vec<String>,

    pub autocorrect_enabled: bool,
    pub autocorrect_on_import: bool,
    pub ac_caps_lock: bool,
    pub ac_capitalise_sentences: bool,
    pub ac_typographic_quotes: bool,
    pub ac_language_single_quotes: bool,
    pub ac_tab_paragraph_level: bool,
    pub ac_two_initial_caps: bool,
    pub ac_superscript_ordinals: bool,
    pub ac_bulleted_lists: bool,
    pub ac_numbered_lists: bool,
    pub ac_replace_while_typing: bool,
    pub ac_replacements: Vec<TextReplacement>,

    pub abbreviation_language: String,
    pub abbreviations: Vec<String>,

    pub filler_language: String,
    pub filler_text: String,
    pub filler_default: String,

    pub title_exception_language: String,
    pub title_exceptions: Vec<String>,

    pub recent_count: u32,
    pub home_show_recent: bool,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn factory_filler() -> String {
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string()
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            reopen_docs: false,
            limit_zoom_100: false,
            open_floating: false,
            open_in_active_group: true,
            auto_update_links: false,
            save_thumbnails: true,
            psd_save_over: false,
            preserve_unicode_breaks: false,
            smart_cut_paste: true,
            filler_as_text: false,
            psd_text_as_text: true,
            haptics: true,
            copy_as_svg: true,
            prefer_metafile_paste: true,
            keep_selection_after_delete: true,
            load_xmp_sidecars: true,
            refine_heic_depth: false,
            apply_gain_maps: false,
            psd_smart_objects: true,
            show_home_on_startup: true,
            send_usage_data: true,
            upload_crash_reports: true,

            ml_models: strings(&["segmentation"]),

            mcp_enabled: true,
            mcp_desktop_files: true,
            mcp_networks: true,
            mcp_use_saved_scripts: true,
            mcp_save_scripts: true,
            mcp_local_hints: true,
            mcp_share_hints: true,

            scripting_enabled: true,
            script_file_system: true,
            script_networks: true,
            script_dynamic_code: true,
            script_folders: Vec::new(),

            rgb_profile: "sRGB IEC61966-2.1".to_string(),
            rgb32_profile: "sRGB IEC61966-2.1 (Linear)".to_string(),
            cmyk_profile: crate::colour::ProfileStore::FOGRA39L.to_string(),
            grey_profile: "Greyscale D50".to_string(),
            lab_profile: "LAB D50".to_string(),
            rendering_intent: "perceptual".to_string(),
            black_point_compensation: true,
            convert_to_working: false,
            convert_warn: true,
            warn_assign_unprofiled: true,
            hdr_32bit_default: false,
            ocio_config: String::new(),
            ocio_by_filename: true,
            ocio_warn_on_load: true,
            exr_associate_alpha: false,
            exr_post_divide: false,
            exr_perturb_zero_alpha: false,

            ram_limit: 8192,
            disk_warn_gb: 5,
            undo_limit: 1024,
            view_quality: "bilinear".to_string(),
            dither_gradients: false,
            precise_clipping: false,
            recovery_interval_s: 300,
            renderer: "default".to_string(),
            retina_rendering: "auto".to_string(),
            opencl: true,
            gpu_device: String::new(),

            artboard_grey: 0.5,
            text_contrast: 0.5,
            accent_colour: String::new(),
            tooltip_delay_ms: 500,
            decimals_px: 0,
            decimals_pt: 1,
            decimals_pc: 1,
            decimals_deg: 1,
            decimals_in: 3,
            decimals_ft: 3,
            decimals_yd: 3,
            decimals_mi: 3,
            decimals_mm: 1,
            decimals_cm: 2,
            decimals_m: 3,
            decimals_km: 3,
            lock_bg_on_import: false,
            default_view_mode: "vector".to_string(),
            lines_in_points: true,
            text_in_points: true,
            show_brush_previews: true,
            always_brush_crosshair: false,
            auto_commit_filters: true,
            pointer_support: false,
            ask_layer_name: false,
            white_font_dropdown: true,
            font_preview_text: "The quick brown fox jumped over the lazy dog".to_string(),
            font_preview_samples: strings(&[
                "The quick brown fox jumped over the lazy dog",
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                "0123456789 ? ! & @ % * ^ # $ / ( ) [ ] { }",
            ]),

            bleed_colour: "#e5004fff".to_string(),
            column_guide_fill_colour: "#4f8fe626".to_string(),
            margins_colour: "#4f8fe6ff".to_string(),
            column_guide_stroke_colour: "#4f8fe680".to_string(),
            guides_colour: "#00b4ffff".to_string(),
            baseline_grid_colour: "#00c0c080".to_string(),
            snapping_grid_colour: "#808080ff".to_string(),
            snapping_grid_subdivision_colour: "#808080ff".to_string(),
            pixel_grid_colour: "#808080ff".to_string(),
            grid_link_defaults: true,
            snapping_grid_opacity: 0.5,
            snapping_grid_subdivision_opacity: 0.25,
            pixel_grid_opacity: 0.25,

            home_items: vec![
                HomeItem { id: "recent".to_string(), visible: true },
                HomeItem { id: "templates".to_string(), visible: true },
                HomeItem { id: "favourites".to_string(), visible: true },
                HomeItem { id: "presets".to_string(), visible: true },
            ],

            shift_cycles_tools: false,
            wheel_zoom: false,
            scrubby_zoom: true,
            dial_support: false,
            sync_tools: true,
            touch_gestures_only: false,
            alt_scroll_rotate: false,
            marquee_behaviour: "intersect".to_string(),
            move_aspect_constrain: "shift_constrains".to_string(),
            tablet_input: "windows_ink".to_string(),
            text_blend_gamma: 1.0,
            nudge_distance: 1.0,
            modifier_nudge_distance: 10.0,
            dictionary_folder: String::new(),

            assistant_enabled: true,
            assistant_auto_undo: true,
            assistant_alert: true,
            assist_paint_no_layer: "new_pixel_layer".to_string(),
            assist_erase_vector: "rasterise".to_string(),
            assist_brush_vector: "new_pixel_layer".to_string(),
            assist_brush_sharing: "share".to_string(),
            assist_filter_non_pixel: "rasterise".to_string(),
            assist_adjustment_selection: "add_child".to_string(),
            assist_mask_selection: "add_child".to_string(),
            assist_filter_layer_selection: "add_child".to_string(),
            assist_delete_image: "rasterise".to_string(),
            assist_duplicate_image: "rasterise".to_string(),

            plugin_folders: Vec::new(),
            allow_unknown_plugins: false,

            linked_services: Vec::new(),

            autocorrect_enabled: false,
            autocorrect_on_import: false,
            ac_caps_lock: true,
            ac_capitalise_sentences: true,
            ac_typographic_quotes: true,
            ac_language_single_quotes: false,
            ac_tab_paragraph_level: true,
            ac_two_initial_caps: true,
            ac_superscript_ordinals: true,
            ac_bulleted_lists: true,
            ac_numbered_lists: true,
            ac_replace_while_typing: true,
            ac_replacements: vec![
                TextReplacement { replace: "(c)".to_string(), with: "\u{a9}".to_string() },
                TextReplacement { replace: "(r)".to_string(), with: "\u{ae}".to_string() },
                TextReplacement { replace: "(tm)".to_string(), with: "\u{2122}".to_string() },
                TextReplacement { replace: "...".to_string(), with: "\u{2026}".to_string() },
            ],

            abbreviation_language: "en_gb".to_string(),
            abbreviations: strings(&[
                "A.D.", "a.v.", "c.", "e.g.", "et al.", "etc.", "i.a.", "i.e.", "ib.", "ibid.",
                "MS.", "N.B.", "P.S.", "Ph. D.",
            ]),

            filler_language: "en_gb".to_string(),
            filler_text: factory_filler(),
            filler_default: factory_filler(),

            title_exception_language: "en_gb".to_string(),
            title_exceptions: strings(&[
                "a", "amid", "an", "and", "anti", "as", "at", "but", "by", "down", "for", "from",
                "in", "into", "like", "near", "nor", "of", "off", "on", "onto", "or", "out",
                "over", "past", "per", "plus", "so", "than", "the", "to", "up", "upon", "via",
                "with", "yet",
            ]),

            recent_count: 10,
            home_show_recent: true,
            home_template_folders: Vec::new(),
        }
    }
}

impl Prefs {
    pub fn recent_cap(&self) -> usize {
        self.recent_count.max(1) as usize
    }

    pub fn factory_filler() -> String {
        factory_filler()
    }

    pub fn default_path() -> PathBuf {
        match std::env::var_os("APPDATA") {
            Some(appdata) => PathBuf::from(appdata).join("DTool").join("prefs.json"),
            None => PathBuf::from("prefs.json"),
        }
    }

    pub fn load(path: &Path) -> Prefs {
        match fs::read_to_string(path) {
            Ok(content) => {
                let mut prefs: Prefs = serde_json::from_str(&content).unwrap_or_default();
                if prefs.accent_colour.eq_ignore_ascii_case("#ff7a00ff") {
                    prefs.accent_colour = String::new();
                }
                prefs
            }
            Err(_) => Prefs::default(),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(path, json)
    }

    pub fn query(&self, keys: Option<&[String]>) -> Result<serde_json::Value, CommandError> {
        let all = serde_json::to_value(self).map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
        match (keys, all) {
            (Some(keys), serde_json::Value::Object(map)) => {
                let mut out = serde_json::Map::new();
                for key in keys {
                    match map.get(key) {
                        Some(value) => {
                            out.insert(key.clone(), value.clone());
                        }
                        None => return Err(CommandError::new("INVALID_PARAMS")),
                    }
                }
                Ok(serde_json::Value::Object(out))
            }
            (_, all) => Ok(all),
        }
    }

    pub fn set(&mut self, partial: &serde_json::Value) -> Result<(), CommandError> {
        let patch = match partial {
            serde_json::Value::Object(map) => map,
            _ => return Err(CommandError::new("INVALID_PARAMS")),
        };
        let mut current = match serde_json::to_value(&*self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return Err(CommandError::new("INTERNAL_ERROR")),
        };
        for (key, value) in patch {
            if !current.contains_key(key) {
                return Err(CommandError::new("INVALID_PARAMS"));
            }
            current.insert(key.clone(), value.clone());
        }
        let next: Prefs = serde_json::from_value(serde_json::Value::Object(current))
            .map_err(|_| CommandError::new("INVALID_PARAMS"))?;
        *self = next;
        Ok(())
    }

    pub fn performance_set(&mut self, partial: &serde_json::Value) -> Result<(), CommandError> {
        let patch = match partial {
            serde_json::Value::Object(map) => map,
            _ => return Err(CommandError::new("INVALID_PARAMS")),
        };
        let mut current = match serde_json::to_value(&*self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return Err(CommandError::new("INTERNAL_ERROR")),
        };
        for (key, value) in patch {
            if !current.contains_key(key) {
                return Err(CommandError::new("INVALID_PARAMS"));
            }
            current.insert(key.clone(), value.clone());
        }
        let next: Prefs = serde_json::from_value(serde_json::Value::Object(current))
            .map_err(|_| CommandError::new("INVALID_PARAMS"))?;
        *self = next;
        Ok(())
    }

    pub fn enumerate_gpus() -> Vec<GpuDevice> {
        enumerate_gpu_devices()
    }

    pub fn run(
        &mut self,
        name: &str,
        params: &serde_json::Value,
        path: &Path,
    ) -> Result<serde_json::Value, CommandError> {
        match name {
            CMD_QUERY => {
                let keys: Option<Vec<String>> = match params.get("keys") {
                    Some(value) => Some(
                        serde_json::from_value(value.clone())
                            .map_err(|_| CommandError::new("INVALID_PARAMS"))?,
                    ),
                    None => None,
                };
                self.query(keys.as_deref())
            }
            CMD_SET => {
                self.set(params)?;
                self.save(path).map_err(|_| CommandError::new("IO_ERROR"))?;
                crate::commands::recent::set_cap(self.recent_cap());
                self.query(None)
            }
            CMD_PERFORMANCE_SET => {
                self.performance_set(params)?;
                self.save(path).map_err(|_| CommandError::new("IO_ERROR"))?;
                crate::commands::recent::set_cap(self.recent_cap());
                self.query(None)
            }
            _ => Err(CommandError::new("UNKNOWN_COMMAND")),
        }
    }
}
