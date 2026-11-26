use std::{collections::HashMap,path::PathBuf,sync::{Mutex,OnceLock}};
use serde::{Serialize,Deserialize};
use crate::dock::{Layout,PanelId};
use crate::tools_panel::{ToolId,default_flyouts};
use dtool_core::{Command,CommandError,CommandResult,Session};

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum PersonaId {Vector,Pixel,Layout,Slice,AiImage,Liquify,Panorama,AstroStack}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ExtraStudio {Typography,Compositing,Retouching,ColourGrading,Astrophotography,Scripting}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(tag="kind",content="id",rename_all="snake_case")]
pub enum PersonaEntry {Builtin(PersonaId),Extra(ExtraStudio),Custom(String)}

impl PersonaEntry {
    pub fn key(&self)->String{match self{Self::Builtin(id)=>serde_json::to_value(id).unwrap().as_str().unwrap().into(),Self::Extra(id)=>serde_json::to_value(id).unwrap().as_str().unwrap().into(),Self::Custom(name)=>format!("custom:{name}")}}
    pub fn parse(id:&str)->Result<Self,CommandError>{
        if let Ok(value)=serde_json::from_value::<PersonaId>(serde_json::json!(id)){return Ok(Self::Builtin(value));}
        if let Ok(value)=serde_json::from_value::<ExtraStudio>(serde_json::json!(id)){return Ok(Self::Extra(value));}
        if let Some(name)=id.strip_prefix("custom:").filter(|s|!s.trim().is_empty()){return Ok(Self::Custom(name.into()));}
        Err(CommandError::invalid_params())
    }
}

#[derive(Debug,Clone)]
pub struct PersonaDef {pub entry:PersonaEntry,pub name_key:String,pub icon:String,pub tools:Vec<ToolId>,pub toolbar:Vec<String>,pub panels:Vec<PanelId>}

pub const DEFAULT_TOOLBAR: [&str; 0] = [];
fn default_toolbar() -> Vec<String> { DEFAULT_TOOLBAR.map(String::from).into() }
const TOOLBAR_ALIGN_GROUP: &str = "toolbar.group.align";
const TOOLBAR_DISTRIBUTE_GROUP: &str = "toolbar.group.distribute";
const TOOLBAR_BOOLEAN_GROUP: &str = "toolbar.group.boolean";
const TOOLBAR_LAYER_GROUP: &str = "toolbar.group.layer";
const TOOLBAR_STACK_GROUP: &str = "toolbar.group.stack";
const TOOLBAR_FLIP_GROUP: &str = "toolbar.group.flip";
const TOOLBAR_HISTORY_GROUP: &str = "toolbar.group.history";
const TOOLBAR_GUIDES_GROUP: &str = "toolbar.group.guides";

pub(crate) fn toolbar_group(id: &str) -> Option<&'static [&'static str]> {
    match id {
        TOOLBAR_ALIGN_GROUP => Some(&[
            "layer.arrange.align.left", "layer.arrange.align.centre", "layer.arrange.align.right",
            "layer.arrange.align.top", "layer.arrange.align.middle", "layer.arrange.align.bottom",
        ]),
        TOOLBAR_DISTRIBUTE_GROUP => Some(&["layer.arrange.distribute.h", "layer.arrange.distribute.v"]),
        TOOLBAR_BOOLEAN_GROUP => Some(&["boolean.add", "boolean.subtract", "boolean.intersect", "boolean.xor", "boolean.divide"]),
        TOOLBAR_LAYER_GROUP => Some(&["node.group", "node.ungroup"]),
        TOOLBAR_STACK_GROUP => Some(&["layer.arrange.move.to_front", "layer.arrange.move.to_back"]),
        TOOLBAR_FLIP_GROUP => Some(&["layer.arrange.flip_horizontal", "layer.arrange.flip_vertical"]),
        TOOLBAR_HISTORY_GROUP => Some(&["edit.undo", "edit.redo"]),
        TOOLBAR_GUIDES_GROUP => Some(&["ruler.toggle", "guides.toggle"]),
        _ => None,
    }
}
pub const TOOLBAR_ACTIONS: &[(&str, &str, &str)] = &[
    (TOOLBAR_HISTORY_GROUP, "cmd-undo", "studio.toolbar.group.history"),
    (TOOLBAR_GUIDES_GROUP, "cmd-guides", "studio.toolbar.group.guides"),
    (TOOLBAR_ALIGN_GROUP, "cmd-align", "studio.toolbar.group.align"),
    (TOOLBAR_DISTRIBUTE_GROUP, "tool-distribute-h", "studio.toolbar.group.distribute"),
    (TOOLBAR_LAYER_GROUP, "cmd-group", "studio.toolbar.group.layer"),
    (TOOLBAR_STACK_GROUP, "cmd-arrange", "studio.toolbar.group.stack"),
    (TOOLBAR_FLIP_GROUP, "cmd-flip", "studio.toolbar.group.flip"),
    (TOOLBAR_BOOLEAN_GROUP, "cmd-bool-add", "studio.toolbar.group.boolean"),
    ("doc.new", "file-new", "menu.file.new"),
    ("doc.open", "file-open", "menu.file.open"),
    ("doc.save", "file-save", "menu.file.save"),
    ("edit.undo", "cmd-undo", "menu.edit.undo"),
    ("edit.redo", "cmd-redo", "menu.edit.redo"),
    ("edit.copy", "edit-copy", "menu.edit.copy"),
    ("edit.duplicate", "edit-duplicate", "menu.edit.duplicate"),
    ("ruler.toggle", "cmd-ruler", "menu.view.show.rulers"),
    ("guides.toggle", "cmd-guides", "menu.view.show.guides"),
    ("vector.convert_to_curves", "tool-shape-curve", "menu.vector.convert_to_curves"),
    (crate::context_bar::align::ALIGN_POPUP_COMMAND, "cmd-align", "studio.toolbar.align_options"),
    ("layer.arrange.align.left", "tool-align-left", "context.move.align.left"),
    ("layer.arrange.align.centre", "tool-align-center-h", "context.move.align.centre"),
    ("layer.arrange.align.right", "tool-align-right", "context.move.align.right"),
    ("layer.arrange.align.top", "tool-align-top", "context.move.align.top"),
    ("layer.arrange.align.middle", "tool-align-center-v", "context.move.align.middle"),
    ("layer.arrange.align.bottom", "tool-align-bottom", "context.move.align.bottom"),
    ("layer.arrange.distribute.h", "tool-distribute-h", "menu.layer.arrange.align.distribute_horizontally"),
    ("layer.arrange.distribute.v", "tool-distribute-v", "menu.layer.arrange.align.distribute_vertically"),
    ("node.group", "cmd-group", "menu.layer.group"),
    ("node.ungroup", "cmd-ungroup", "menu.layer.ungroup"),
    ("layer.arrange.move.to_front", "cmd-arrange-front", "menu.layer.arrange.move.to_front"),
    ("layer.arrange.move.to_back", "cmd-arrange-back", "menu.layer.arrange.move.to_back"),
    ("layer.arrange.flip_horizontal", "cmd-flip", "menu.layer.arrange.flip_horizontal"),
    ("layer.arrange.flip_vertical", "cmd-flip-vertical", "menu.layer.arrange.flip_vertical"),
    ("boolean.add", "cmd-bool-add", "menu.vector.geometry.add"),
    ("boolean.subtract", "cmd-bool-subtract", "menu.vector.geometry.subtract"),
    ("boolean.intersect", "cmd-bool-intersect", "menu.vector.geometry.intersect"),
    ("boolean.xor", "cmd-bool-xor", "menu.vector.geometry.xor"),
    ("boolean.divide", "cmd-bool-divide", "menu.vector.geometry.divide"),
];

fn registered_tools()->Vec<ToolId>{default_flyouts().into_iter().flat_map(|flyout|std::iter::once(flyout.parent).chain(flyout.children)).collect()}
fn tool(id:&str)->Option<ToolId>{
    dtool_core::context_toolbar::TOOL_IDS.iter().copied().find(|known| *known == id).map(ToolId)
        .or_else(|| registered_tools().into_iter().find(|tool| tool.0 == id))
}

fn layout_publication_tool(id: &str) -> bool {
    id == "tool.table" || id.starts_with("tool.picture_frame")
}

pub fn persona_catalog()->Vec<PersonaDef>{
    let mut result=Vec::new();
    for id in [PersonaId::Vector,PersonaId::Pixel,PersonaId::Layout,PersonaId::Slice,PersonaId::AiImage,PersonaId::Liquify,PersonaId::Panorama,PersonaId::AstroStack]{
        let entry=PersonaEntry::Builtin(id);let key=entry.key();
        let panels=match id{
            PersonaId::Layout | PersonaId::Vector | PersonaId::Pixel => {
                let persona = crate::presets::WorkspacePersona::from_key(&key).unwrap();
                crate::presets::default_layout(persona).panels.into_iter().map(|p| p.id).collect()
            },
            PersonaId::AiImage=>vec![PanelId::Layers,PanelId::Other(crate::panels::AI_IMAGE_PANEL_KEY.into())],
            PersonaId::Slice=>vec![PanelId::Layers,PanelId::Other(crate::panels::SLICES_PANEL_KEY.into())],
            _=>vec![PanelId::Layers,PanelId::Colour,PanelId::History,PanelId::Navigator],
        };
        let tools=registered_tools().into_iter().filter(|tool|match id{
            _ if tool.0 == "tool.paint.pixel" => id == PersonaId::Pixel,
            _ if matches!(tool.0, "tool.select.object" | "tool.selection_brush" | "tool.magic_wand") => id == PersonaId::Pixel,
            _ if matches!(tool.0, "tool.paint.adjustment" | "tool.paint.tone" | "tool.dodge" | "tool.burn" | "tool.paint.sponge"
                | "tool.paint.filter" | "tool.paint.blur" | "tool.sharpen" | "tool.paint.median" | "tool.paint.smudge") => id == PersonaId::Pixel,
            PersonaId::Vector=>!tool.0.starts_with("tool.eraser")&&!tool.0.starts_with("tool.marquee")&&tool.0!="tool.crop"&&tool.0!="tool.flood_fill",
            _ if matches!(tool.0, "tool.shape_builder" | "tool.area" | "tool.vector_crop" | "tool.stroke_width" | "tool.qr" | "tool.artboard") => false,
            PersonaId::Slice=>["tool.view","tool.zoom","tool.move","tool.slice"].contains(&tool.0),
            PersonaId::Layout=>layout_publication_tool(tool.0)||tool.0.starts_with("tool.text")||tool.0.starts_with("tool.shap")||["tool.view","tool.zoom","tool.move"].contains(&tool.0),
            _=>true,
        }).collect();
        result.push(PersonaDef{entry,name_key:format!("persona.{key}"),icon:format!("persona.{key}"),tools,toolbar:default_toolbar(),panels});
    }
    for id in [ExtraStudio::Typography,ExtraStudio::Compositing,ExtraStudio::Retouching,ExtraStudio::ColourGrading,ExtraStudio::Astrophotography,ExtraStudio::Scripting]{
        let entry=PersonaEntry::Extra(id);let key=entry.key();
        let (tools,toolbar,panels,name_key)=match id{
            ExtraStudio::Typography=>(registered_tools().into_iter().filter(|t|t.0.starts_with("tool.text")||["tool.view","tool.zoom","tool.move"].contains(&t.0)).collect(),vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Character,PanelId::Paragraph,PanelId::Layers,PanelId::History],format!("studio.{key}")),
            ExtraStudio::Compositing=>(registered_tools().into_iter().filter(|t|["tool.view","tool.zoom","tool.move","tool.selection","tool.paint","tool.eraser"].contains(&t.0)).collect(),vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Layers,PanelId::Colour,PanelId::History,PanelId::Navigator],format!("studio.{key}")),
            ExtraStudio::Retouching=>(registered_tools().into_iter().filter(|t|{let id=t.0;id=="tool.move"||id=="tool.selection"||id=="tool.crop"||id=="tool.flood_fill"||id.starts_with("tool.paint")||id.starts_with("tool.eraser")||id.starts_with("tool.clone")||id.starts_with("tool.heal")||id.starts_with("tool.dodge")||id.starts_with("tool.burn")||id.starts_with("tool.sharpen")||id.starts_with("tool.soften")||id.starts_with("tool.smudge")||id=="tool.view"||id=="tool.zoom"}).collect(),vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Layers,PanelId::Colour,PanelId::History,PanelId::Navigator],"studio.retouching".to_string()),
            ExtraStudio::ColourGrading=>(registered_tools().into_iter().filter(|t|{let id=t.0;id=="tool.view"||id=="tool.zoom"||id=="tool.move"||id=="tool.adjustment"||id.starts_with("tool.curves")||id.starts_with("tool.levels")||id.starts_with("tool.exposure")||id.starts_with("tool.white_balance")||id.starts_with("tool.hsl")||id.starts_with("tool.color_balance")||id.starts_with("tool.black_and_white")||id.starts_with("tool.invert")||id.starts_with("tool.posterize")||id.starts_with("tool.threshold")||id.starts_with("tool.gradient_map")}).collect(),vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Colour,PanelId::Swatches,PanelId::Layers,PanelId::History,PanelId::Navigator],format!("studio.{key}")),
            ExtraStudio::Astrophotography=>(registered_tools().into_iter().filter(|t|["tool.view","tool.zoom","tool.move"].contains(&t.0)).collect(),vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Layers,PanelId::Colour,PanelId::History,PanelId::Navigator],format!("studio.{key}")),
            ExtraStudio::Scripting=>{
                let scripting_order=["tool.view","tool.zoom","tool.move","tool.node","tool.text","tool.shape","tool.pen","tool.line","tool.curve","tool.rectangle","tool.ellipse","tool.polygon","tool.star","tool.spiral","tool.slice","tool.crop","tool.flood_fill","tool.paint_brush","tool.eraser","tool.clone","tool.heal","tool.dodge","tool.burn","tool.sharpen","tool.soften","tool.smudge"];
                let tools=scripting_order.iter().filter_map(|id|tool(id)).collect();
                (tools,vec!["edit.undo".into(),"edit.redo".into()],vec![PanelId::Layers,PanelId::Properties,PanelId::History,PanelId::Navigator],format!("studio.{key}"))
            },
        };
        result.push(PersonaDef{entry,name_key,icon:format!("persona.{key}"),tools,toolbar,panels});
    }
    result
}

#[derive(Debug,Clone)]
pub struct PersonaLayout {pub panels:Vec<PanelId>,pub dock:serde_json::Value,pub tool:Option<ToolId>,pub tools:Vec<ToolId>,pub toolbar:Vec<String>,pub tool_columns:Option<u8>,pub standalone_tools:Vec<ToolId>}
#[derive(Serialize,Deserialize)]
struct LayoutWire {panels:Vec<PanelId>,dock:serde_json::Value,tool:Option<String>,#[serde(default)]tools:Vec<String>,#[serde(default="default_toolbar")]toolbar:Vec<String>,#[serde(default)]tool_columns:Option<u8>,#[serde(default,skip_serializing_if="Vec::is_empty")]standalone_tools:Vec<String>}
impl Serialize for PersonaLayout {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error>{LayoutWire{panels:self.panels.clone(),dock:self.dock.clone(),tool:self.tool.as_ref().map(|tool|tool.0.into()),tools:self.tools.iter().map(|tool|tool.0.into()).collect(),toolbar:self.toolbar.clone(),tool_columns:self.tool_columns,standalone_tools:self.standalone_tools.iter().map(|tool|tool.0.into()).collect()}.serialize(serializer)}
}
impl<'de> Deserialize<'de> for PersonaLayout {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
        let value=LayoutWire::deserialize(deserializer)?;
        let active=value.tool.and_then(|id|tool(&id));
        let tools=value.tools.into_iter().filter_map(|id|tool(&id)).collect::<Vec<_>>();
        let mut panels=Vec::new();
        for id in value.panels {
            let id=crate::panel_ids::canonical(id);
            if !panels.iter().any(|known: &PanelId| known.key()==id.key()) { panels.push(id); }
        }
        Ok(Self{panels,dock:value.dock,tool:active,tools,toolbar:value.toolbar,tool_columns:value.tool_columns.map(|count| count.clamp(1, 4)),standalone_tools:value.standalone_tools.into_iter().filter_map(|id|tool(&id)).collect()})
    }
}
fn initial(definition:&PersonaDef)->PersonaLayout {
    let mut layout=crate::presets::WorkspacePersona::from_key(&definition.entry.key())
        .filter(|p| matches!(p, crate::presets::WorkspacePersona::Vector | crate::presets::WorkspacePersona::Pixel | crate::presets::WorkspacePersona::Layout))
        .map(crate::presets::default_layout).unwrap_or_default();
    let hidden_preset_panels: Vec<_> = layout.panels.iter()
        .filter(|panel| panel.visible && !panel.id.is_named() && !definition.panels.contains(&panel.id))
        .map(|panel| panel.id.clone())
        .collect();
    for panel in hidden_preset_panels {
        layout.set_panel_visible(&panel, false);
    }
    for panel in crate::panel_ids::NAMED_PANELS {layout.set_panel_visible(&panel,definition.panels.contains(&panel));}
    for panel in &definition.panels{layout.set_panel_visible(panel,true);}
    PersonaLayout{panels:definition.panels.clone(),dock:serde_json::to_value(layout).unwrap_or(serde_json::Value::Null),tool:tool("tool.move"),tools:definition.tools.clone(),toolbar:definition.toolbar.clone(),tool_columns:None,standalone_tools:Vec::new()}
}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct PersonaState {
    pub active: PersonaEntry,
    pub layouts: HashMap<String, PersonaLayout>,
    pub live: PersonaLayout,
    #[serde(default)]
    pub custom_studios: Vec<CustomStudio>,
    #[serde(default)]
    toolbar_defaults_updated: bool,
}
impl Default for PersonaState {
    fn default()->Self{let def=persona_catalog().remove(0);let live=initial(&def);Self{active:def.entry,layouts:HashMap::new(),live,custom_studios:Vec::new(),toolbar_defaults_updated:true}}
}

pub fn switch_persona(state:&mut PersonaState,to:PersonaEntry)->Result<(),CommandError>{
    if state.active==to{return Ok(());}
    let next = if let Some(saved) = state.layouts.get(&to.key()) {
        saved.clone()
    } else if let PersonaEntry::Custom(name) = &to {
        let studio = state.custom_studios.iter().find(|studio| &studio.name == name).ok_or_else(CommandError::not_found)?;
        initial_studio(studio)
    } else {
        initial(&persona_catalog().into_iter().find(|definition| definition.entry == to).ok_or_else(CommandError::not_found)?)
    };
    let _:Layout=serde_json::from_value(next.dock.clone()).map_err(|_|CommandError::invalid_params())?;
    state.layouts.insert(state.active.key(),state.live.clone());state.active=to;state.live=next;Ok(())
}

pub fn refuse_excluded(entry:&PersonaEntry)->Result<(),CommandError>{
    match entry{PersonaEntry::Extra(_)|PersonaEntry::Builtin(PersonaId::Liquify)=>Err(CommandError::not_implemented()),_=>Ok(())}
}

pub fn persona_status(entry:&PersonaEntry)->&'static str{if refuse_excluded(entry).is_err(){"planned"}else{"supported"}}

fn refresh_legacy_flyout(layout: &mut PersonaLayout, flyout: &crate::tools_panel::ToolFlyout, is_new: impl Fn(&str) -> bool) {
    if !layout.tools.contains(&flyout.parent) {
        return;
    }
    let legacy = flyout.children.iter().filter(|tool| !is_new(tool.0));
    let existing = layout.tools.iter().filter(|tool| flyout.children.contains(tool));
    if !existing.eq(legacy) {
        return;
    }
    let index = layout.tools.iter().rposition(|tool| flyout.children.contains(tool))
        .expect("matched a legacy flyout with children") + 1;
    layout.tools.splice(index..index, flyout.children.iter().filter(|tool| is_new(tool.0)).cloned());
}

#[derive(Debug,Clone)]
pub struct PersonaStore {pub dir:PathBuf}
impl PersonaStore {
    pub fn load(&self) -> PersonaState {
        let mut state: PersonaState = std::fs::read(self.dir.join("state.json")).ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        if !state.toolbar_defaults_updated {
            let old_defaults = ["edit.undo", "edit.redo"];
            for (key, layout) in &mut state.layouts {
                if !key.starts_with("custom:") && layout.toolbar == old_defaults {
                    layout.toolbar.clear();
                }
            }
            if matches!(state.active, PersonaEntry::Builtin(_)) && state.live.toolbar == old_defaults {
                state.live.toolbar.clear();
            }
            state.toolbar_defaults_updated = true;
        }
        let flyouts = default_flyouts();
        let shapes = flyouts.iter().find(|group| group.parent.0 == "tool.shapes").unwrap();
        let boolean = flyouts.iter().find(|group| group.parent.0 == "tool.boolean").unwrap();
        let live_builtin = matches!(state.active, PersonaEntry::Builtin(_));
        let saved = state.layouts.iter_mut().filter(|(key, _)| !key.starts_with("custom:")).map(|(_, layout)| layout);
        for layout in saved.chain(live_builtin.then_some(&mut state.live)) {
            refresh_legacy_flyout(layout, shapes, dtool_core::context_toolbar::is_advanced_shape_tool);
            refresh_legacy_flyout(layout, boolean, |id| id == "tool.boolean.divide");
            if layout.tool.as_ref().is_some_and(|tool| crate::tools_panel::boolean_tool_command(tool.0).is_some()) {
                layout.tool = Some(ToolId("tool.move"));
            }
        }
        if let Some(definition) = persona_catalog().into_iter()
            .find(|definition| definition.entry == PersonaEntry::Builtin(PersonaId::Layout))
        {
            let active_layout = state.active == definition.entry;
            let legacy_tools: Vec<_> = definition.tools.iter()
                .filter(|tool| !layout_publication_tool(tool.0)
                    && !dtool_core::context_toolbar::is_advanced_shape_tool(tool.0)).cloned().collect();
            for layout in state.layouts.get_mut("layout").into_iter().chain(active_layout.then_some(&mut state.live)) {
                if layout.tool_columns.is_none() && layout.standalone_tools.is_empty() && layout.tools == legacy_tools {
                    layout.tools.clone_from(&definition.tools);
                }
            }
        }
        let active_vector = state.active == PersonaEntry::Builtin(PersonaId::Vector);
        let vector_layouts = state.layouts.get_mut("vector").into_iter()
            .chain(active_vector.then_some(&mut state.live));
        for layout in vector_layouts {
            if layout.tool_columns.is_some() { continue; }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.artboard") {
                let index = layout.tools.iter().position(|tool| tool.0 == "tool.move").map_or(0, |index| index + 1);
                layout.tools.insert(index, ToolId("tool.artboard"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.qr") {
                let index = layout.tools.iter().position(|tool| tool.0 == "tool.shapes").map_or(layout.tools.len(), |index| index + 1);
                layout.tools.insert(index, ToolId("tool.qr"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.stroke_width") {
                let index = layout.tools.iter().position(|tool| tool.0 == "tool.pen").map_or(layout.tools.len(), |index| index + 1);
                layout.tools.insert(index, ToolId("tool.stroke_width"));
            }
            let has_builder = layout.tools.iter().any(|tool| tool.0 == "tool.shape_builder");
            if !has_builder {
                layout.tools.push(ToolId("tool.shape_builder"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.style_picker") {
                layout.tools.push(ToolId("tool.style_picker"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.measure") {
                layout.tools.push(ToolId("tool.measure"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.area") {
                let index = layout.tools.iter().position(|tool| tool.0 == "tool.measure").map_or(layout.tools.len(), |index| index + 1);
                layout.tools.insert(index, ToolId("tool.area"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.transparency")
                && let Some(index) = layout.tools.iter().position(|tool| tool.0 == "tool.gradient")
            {
                layout.tools.insert(index + 1, ToolId("tool.transparency"));
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.vector_crop") {
                layout.tools.push(ToolId("tool.vector_crop"));
            }
            if layout.tools.iter().any(|tool| tool.0 == "tool.transform")
                && !layout.tools.iter().any(|tool| tool.0 == "tool.point_transform")
            {
                layout.tools.push(ToolId("tool.point_transform"));
            }
        }
        let active_pixel = state.active == PersonaEntry::Builtin(PersonaId::Pixel);
        for layout in state.layouts.get_mut("pixel").into_iter().chain(active_pixel.then_some(&mut state.live)) {
            if layout.tool_columns.is_some() { continue; }
            let marquee = [
                ToolId("tool.marquee"), ToolId("tool.marquee.rectangle"), ToolId("tool.marquee.ellipse"),
                ToolId("tool.marquee.row"), ToolId("tool.marquee.column"),
            ];
            if let Some(index) = layout.tools.windows(marquee.len()).position(|group| group == marquee) {
                if !layout.tools.contains(&ToolId("tool.marquee.freehand")) {
                    layout.tools.insert(index + marquee.len(), ToolId("tool.marquee.freehand"));
                }
            } else if !layout.tools.iter().any(|tool| tool.0.starts_with("tool.marquee")) {
                let index = layout.tools.iter().position(|tool| tool.0 == "tool.move").map_or(0, |index| index + 1);
                layout.tools.splice(index..index, marquee.into_iter().chain([ToolId("tool.marquee.freehand")]));
            }
            let erasers = [ToolId("tool.eraser"), ToolId("tool.eraser.erase"), ToolId("tool.eraser.smudge")];
            let flood_eraser = ToolId("tool.eraser.flood");
            if !layout.tools.contains(&flood_eraser)
                && let Some(index) = layout.tools.windows(erasers.len()).position(|group| group == erasers)
            {
                layout.tools.insert(index + erasers.len(), flood_eraser);
            }
            let background_eraser = ToolId("tool.eraser.background");
            if !layout.tools.contains(&background_eraser)
                && layout.tools.iter().any(|tool| tool.0 == "tool.eraser.flood")
                && let Some(index) = layout.tools.iter().position(|tool| tool.0 == "tool.eraser.erase")
            {
                layout.tools.insert(index + 1, background_eraser);
            }
            let mixer = ToolId("tool.paint.mixer");
            if !layout.tools.contains(&mixer)
                && let Some(index) = layout.tools.windows(2).position(|tools| {
                    tools[0].0 == "tool.paint.brush" && matches!(tools[1].0, "tool.paint.colour_replace" | "tool.paint.pencil")
                })
            {
                layout.tools.insert(index + 1, mixer);
            }
            let colour_replace = ToolId("tool.paint.colour_replace");
            if !layout.tools.contains(&colour_replace)
                && let Some(index) = layout.tools.windows(2).position(|tools| {
                    matches!(tools[0].0, "tool.paint.brush" | "tool.paint.mixer") && tools[1].0 == "tool.paint.pencil"
                })
            {
                layout.tools.insert(index + 1, colour_replace);
            }
            let pixel_brush = ToolId("tool.paint.pixel");
            if !layout.tools.contains(&pixel_brush)
                && let Some(index) = layout.tools.windows(2).position(|tools| {
                    tools[0].0 == "tool.paint.pencil" && tools[1].0 == "tool.paint.vector_brush"
                })
            {
                layout.tools.insert(index + 1, pixel_brush);
            }
            let tonal_brushes = [ToolId("tool.paint.adjustment"), ToolId("tool.dodge"), ToolId("tool.burn"), ToolId("tool.paint.sponge")];
            if !layout.tools.iter().any(|tool| tonal_brushes.contains(tool))
                && let Some(index) = layout.tools.iter().position(|tool| tool.0 == "tool.symmetry")
                && layout.tools.iter().any(|tool| tool.0 == "tool.paint_brush")
            {
                layout.tools.splice(index..index, tonal_brushes);
            }
            if !layout.tools.iter().any(|tool| tool.0 == "tool.paint.tone")
                && let Some(index) = layout.tools.windows(2).position(|tools| tools[0].0 == "tool.paint.adjustment" && tools[1].0 == "tool.dodge")
            {
                layout.tools.insert(index + 1, ToolId("tool.paint.tone"));
            }
            let filter_brushes = [
                ToolId("tool.paint.filter"), ToolId("tool.paint.blur"), ToolId("tool.sharpen"),
                ToolId("tool.paint.median"), ToolId("tool.paint.smudge"),
            ];
            if !layout.tools.iter().any(|tool| filter_brushes.contains(tool))
                && let Some(index) = layout.tools.iter().position(|tool| tool.0 == "tool.symmetry")
                && layout.tools.iter().any(|tool| tool.0 == "tool.paint_brush")
            {
                layout.tools.splice(index..index, filter_brushes);
            }
            let selection = [ToolId("tool.select.object"), ToolId("tool.selection_brush"), ToolId("tool.magic_wand")];
            if !layout.tools.iter().any(|tool| selection.contains(tool))
                && let Some(index) = layout.tools.iter().position(|tool| tool.0 == "tool.marquee")
            {
                layout.tools.splice(index..index, selection);
            }
        }
        for definition in persona_catalog().into_iter().filter(|definition| {
            matches!(definition.entry, PersonaEntry::Builtin(PersonaId::Vector | PersonaId::Layout | PersonaId::Pixel))
        }) {
            let legacy_tools: Vec<_> = definition.tools.iter().filter(|tool| {
                !dtool_core::context_toolbar::is_advanced_shape_tool(tool.0)
            }).cloned().collect();
            let active = state.active == definition.entry;
            for layout in state.layouts.get_mut(&definition.entry.key()).into_iter().chain(active.then_some(&mut state.live)) {
                if layout.tool_columns.is_none() && layout.standalone_tools.is_empty() && layout.tools == legacy_tools {
                    layout.tools.clone_from(&definition.tools);
                }
            }
        }
        state
    }
    pub fn save(&self,state:&PersonaState)->std::io::Result<()>{
        let bytes=serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;std::fs::create_dir_all(&self.dir)?;
        let path=self.dir.join("state.json");let temporary=self.dir.join("state.json.tmp");
        std::fs::write(&temporary,bytes)?;std::fs::rename(temporary,path)
    }
}

#[derive(Debug,Clone)]
pub struct SubPersonaSession {pub id:PersonaId,pub previous:PersonaEntry,pub doc_version_at_enter:u64}

#[derive(Debug,Clone)]
pub struct CustomStudio {pub name:String,pub base:PersonaId,pub tools:Vec<ToolId>,pub toolbar:Vec<String>,pub panels:Vec<PanelId>}
#[derive(Serialize,Deserialize)]
struct CustomStudioWire {name:String,base:PersonaId,#[serde(default)]tools:Vec<String>,#[serde(default="default_toolbar")]toolbar:Vec<String>,#[serde(default)]panels:Vec<PanelId>}
impl Serialize for CustomStudio {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error>{CustomStudioWire{name:self.name.clone(),base:self.base,tools:self.tools.iter().map(|tool|tool.0.into()).collect(),toolbar:self.toolbar.clone(),panels:self.panels.clone()}.serialize(serializer)}
}
impl<'de> Deserialize<'de> for CustomStudio {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
        let value=CustomStudioWire::deserialize(deserializer)?;
        let tools=value.tools.iter().filter_map(|id|tool(id)).collect();
        let allowed = crate::panel_ids::studio_panel_ids();
        let mut panels = Vec::new();
        for id in value.panels.into_iter().map(crate::panel_ids::canonical) {
            if allowed.contains(&id) && !panels.contains(&id) {
                panels.push(id);
            }
        }
        Ok(Self{name:value.name,base:value.base,tools,toolbar:value.toolbar,panels})
    }
}

#[derive(Serialize, Deserialize)]
struct StudioFile {
    #[serde(flatten)]
    studio: CustomStudio,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_appearance: Option<StudioToolAppearance>,
}

#[derive(Serialize, Deserialize)]
struct StudioToolAppearance {
    columns: u8,
    #[serde(default)]
    standalone: Vec<String>,
}

impl StudioToolAppearance {
    fn from_layout(layout: &PersonaLayout) -> Option<Self> {
        if layout.tool_columns.is_none() && layout.standalone_tools.is_empty() {
            return None;
        }
        Some(Self {
            columns: layout.tool_columns.unwrap_or(1),
            standalone: layout.standalone_tools.iter().map(|tool| tool.0.to_owned()).collect(),
        })
    }

    fn apply(self, layout: &mut PersonaLayout) {
        layout.tool_columns = Some(self.columns.clamp(1, 4));
        layout.standalone_tools = self.standalone.iter()
            .filter_map(|id| tool(id)).filter(|id| layout.tools.contains(id)).collect();
    }
}

fn write_studio(studio: &CustomStudio, layout: Option<&PersonaLayout>, path: &std::path::Path) -> Result<(), CommandError> {
    let file = StudioFile { studio: studio.clone(), tool_appearance: layout.and_then(StudioToolAppearance::from_layout) };
    let bytes=serde_json::to_vec_pretty(&file).map_err(|_|CommandError::new("IO_ERROR"))?;
    std::fs::write(path,bytes).map_err(|_|CommandError::new("IO_ERROR"))?;
    Ok(())
}

pub fn export_studio(studio:&CustomStudio,path:&std::path::Path)->Result<(),CommandError>{
    write_studio(studio, None, path)
}

fn read_studio(path: &std::path::Path) -> Result<StudioFile, CommandError> {
    let bytes=std::fs::read(path).map_err(|_|CommandError::not_found())?;
    serde_json::from_slice(&bytes).map_err(|_|CommandError::invalid_params())
}

pub fn import_studio(path:&std::path::Path)->Result<CustomStudio,CommandError>{
    read_studio(path).map(|file| file.studio)
}

pub fn studio_def(studio:&CustomStudio)->PersonaDef{
    PersonaDef{entry:PersonaEntry::Custom(studio.name.clone()),name_key:"studio.custom".into(),icon:format!("persona.{}", PersonaEntry::Builtin(studio.base).key()),tools:studio.tools.clone(),toolbar:studio.toolbar.clone(),panels:studio.panels.clone()}
}

fn initial_studio(studio: &CustomStudio) -> PersonaLayout {
    let mut definition = studio_def(studio);
    definition.entry = PersonaEntry::Builtin(studio.base);
    initial(&definition)
}

pub(crate) const STUDIO_BASES: [PersonaId; 3] = [PersonaId::Vector, PersonaId::Pixel, PersonaId::Layout];

pub fn studio_base(base:Option<&str>)->Result<PersonaId,CommandError>{
    match PersonaEntry::parse(base.unwrap_or("vector"))?{
        PersonaEntry::Builtin(id) if STUDIO_BASES.contains(&id) => Ok(id),
        PersonaEntry::Builtin(PersonaId::Liquify|PersonaId::Panorama|PersonaId::AstroStack|PersonaId::AiImage)|PersonaEntry::Extra(_)=>Err(CommandError::not_implemented()),
        _=>Err(CommandError::invalid_params()),
    }
}

pub fn studio_tools(base:PersonaId,names:&[serde_json::Value],skipped:&mut Vec<String>)->Vec<ToolId>{
    let allowed=persona_catalog().into_iter().find(|def|def.entry==PersonaEntry::Builtin(base)).map(|def|def.tools).unwrap_or_default();
    let mut tools=Vec::new();
    for value in names{
        let name=value.as_str().unwrap_or_default();
        match allowed.iter().find(|tool|tool.0==name){
            Some(tool) if !tools.iter().any(|kept:&ToolId|kept.0==tool.0)=>tools.push(tool.clone()),
            Some(_)=>{}
            None=>skipped.push(name.to_string()),
        }
    }
    tools
}

pub fn studio_panels(names: &[serde_json::Value], skipped: &mut Vec<String>) -> Vec<PanelId> {
    let allowed = crate::panel_ids::studio_panel_ids();
    let mut panels = Vec::new();
    for value in names {
        let name = value.as_str().unwrap_or_default();
        let panel = PanelId::from_key(name);
        if !allowed.contains(&panel) {
            skipped.push(name.to_string());
        } else if !panels.contains(&panel) {
            panels.push(panel);
        }
    }
    panels
}

fn studio_name(params:&serde_json::Value,key:&str)->Result<String,CommandError>{
    let name=params.get(key).and_then(|v|v.as_str()).map(str::trim).unwrap_or_default();
    if name.is_empty()||name.contains(':'){return Err(CommandError::invalid_params());}
    Ok(name.to_string())
}

pub fn create_studio(state:&mut PersonaState,params:&serde_json::Value)->Result<(CustomStudio,Vec<String>),CommandError>{
    let name=studio_name(params,"name")?;
    let base=studio_base(params.get("base").and_then(|v|v.as_str()))?;
    if state.custom_studios.iter().any(|studio|studio.name==name){return Err(CommandError::new("CONFLICT"));}
    let def=persona_catalog().into_iter().find(|d|d.entry==PersonaEntry::Builtin(base)).ok_or_else(CommandError::not_found)?;
    let mut skipped=Vec::new();
    let tools=match params.get("tools").and_then(|v|v.as_array()){Some(list)=>studio_tools(base,list,&mut skipped),None=>def.tools.clone()};
    let panels=match params.get("panels").and_then(|v|v.as_array()){Some(list)=>studio_panels(list,&mut skipped),None=>def.panels.clone()};
    let toolbar=params.get("toolbar").and_then(|v|v.as_array()).map(|list|list.iter().filter_map(|t|t.as_str()).map(String::from).collect()).unwrap_or_else(||def.toolbar.clone());
    let studio=CustomStudio{name,base,tools,toolbar,panels};
    state.custom_studios.push(studio.clone());
    Ok((studio,skipped))
}

pub fn update_studio(state:&mut PersonaState,params:&serde_json::Value)->Result<Vec<String>,CommandError>{
    let name=studio_name(params,"name")?;
    let studio=state.custom_studios.iter_mut().find(|studio|studio.name==name).ok_or_else(CommandError::not_found)?;
    let previous = studio.clone();
    let mut skipped=Vec::new();
    if let Some(list)=params.get("tools").and_then(|v|v.as_array()){studio.tools=studio_tools(studio.base,list,&mut skipped);}
    if let Some(list)=params.get("toolbar").and_then(|v|v.as_array()){studio.toolbar=list.iter().filter_map(|t|t.as_str()).map(String::from).collect();}
    if let Some(list)=params.get("panels").and_then(|v|v.as_array()){studio.panels=studio_panels(list,&mut skipped);}
    let entry = PersonaEntry::Custom(name);
    let layout = if state.active == entry { Some(&mut state.live) } else { state.layouts.get_mut(&entry.key()) };
    if let Some(layout) = layout {
        apply_studio_changes(layout, &previous, studio)?;
    }
    Ok(skipped)
}

fn apply_studio_changes(layout: &mut PersonaLayout, previous: &CustomStudio, studio: &CustomStudio) -> Result<(), CommandError> {
    if previous.toolbar != studio.toolbar {
        layout.toolbar = studio.toolbar.clone();
    }
    if !previous.tools.iter().map(|tool| tool.0).eq(studio.tools.iter().map(|tool| tool.0)) {
        layout.tools = studio.tools.clone();
        if !layout.tool.as_ref().is_some_and(|active| layout.tools.iter().any(|tool| tool.0 == active.0)) {
            layout.tool = layout.tools.first().cloned();
        }
    }
    if previous.panels != studio.panels {
        let mut dock: Layout = serde_json::from_value(layout.dock.clone()).map_err(|_| CommandError::invalid_params())?;
        for panel in previous.panels.iter().filter(|id| !studio.panels.contains(id)) {
            dock.set_panel_visible(panel, false);
        }
        for panel in studio.panels.iter().filter(|id| !previous.panels.contains(id)) {
            dock.set_panel_visible(panel, true);
        }
        layout.panels = dock.panels.iter().filter(|panel| panel.visible).map(|panel| panel.id.clone()).collect();
        layout.dock = serde_json::to_value(dock).map_err(|_| CommandError::invalid_params())?;
    }
    Ok(())
}

pub fn rename_studio(state:&mut PersonaState,params:&serde_json::Value)->Result<(),CommandError>{
    let old=studio_name(params,"name")?;
    let new=studio_name(params,"new_name")?;
    if old==new{return Ok(());}
    if state.custom_studios.iter().any(|studio|studio.name==new){return Err(CommandError::new("CONFLICT"));}
    let studio=state.custom_studios.iter_mut().find(|studio|studio.name==old).ok_or_else(CommandError::not_found)?;
    studio.name=new.clone();
    if let Some(layout)=state.layouts.remove(&PersonaEntry::Custom(old.clone()).key()){state.layouts.insert(PersonaEntry::Custom(new.clone()).key(),layout);}
    if state.active==PersonaEntry::Custom(old){state.active=PersonaEntry::Custom(new);}
    Ok(())
}

pub fn delete_studio(state:&mut PersonaState,params:&serde_json::Value)->Result<(),CommandError>{
    let name=studio_name(params,"name")?;
    let index=state.custom_studios.iter().position(|studio|studio.name==name).ok_or_else(CommandError::not_found)?;
    let entry=PersonaEntry::Custom(name);
    if state.active == entry {
        switch_persona(state, PersonaEntry::Builtin(state.custom_studios[index].base))?;
    }
    state.custom_studios.remove(index);
    state.layouts.remove(&entry.key());
    Ok(())
}

pub fn host_action(command:&Command)->CommandResult{shared_action(command)}

pub fn save_toolbar(workspace: &str, toolbar: &[String]) -> Result<(), CommandError> {
    let mut host = HOST.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?
        .lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
    if host.state.active.key() != workspace {
        return Err(CommandError::new("CONFLICT"));
    }
    let mut candidate = host.state.clone();
    candidate.live.toolbar = toolbar.to_vec();
    if let PersonaEntry::Custom(name) = &candidate.active {
        let studio = candidate.custom_studios.iter_mut().find(|item| &item.name == name).ok_or_else(CommandError::not_found)?;
        studio.toolbar = toolbar.to_vec();
    }
    host.store.save(&candidate).map_err(|_| CommandError::new("IO_ERROR"))?;
    host.state = candidate;
    host.revision += 1;
    Ok(())
}

pub fn refuse_planned(entry:&PersonaEntry)->Result<(),CommandError>{
    match entry{PersonaEntry::Extra(_)|PersonaEntry::Builtin(PersonaId::Liquify)=>Err(CommandError::not_implemented()),_=>Ok(())}
}

pub fn sub_target(entry:&PersonaEntry)->Result<PersonaId,CommandError>{
    refuse_planned(entry)?;
    match entry{
        PersonaEntry::Builtin(id@(PersonaId::Panorama|PersonaId::AstroStack|PersonaId::AiImage))=>Ok(*id),
        _=>Err(CommandError::invalid_params()),
    }
}

pub fn enter_sub_state(state:&mut PersonaState,sub:&mut Option<SubPersonaSession>,store:&PersonaStore,id:&str,doc_version:u64)->Result<(),CommandError>{
    let entry=PersonaEntry::parse(id)?;
    let target=sub_target(&entry)?;
    if sub.is_some(){return Err(CommandError::new("ALREADY_IN_SUB_PERSONA"));}
    let previous=state.active.clone();
    let mut candidate=state.clone();switch_persona(&mut candidate,entry)?;
    store.save(&candidate).map_err(|_|CommandError::new("IO_ERROR"))?;
    *state=candidate;
    *sub=Some(SubPersonaSession{id:target,previous,doc_version_at_enter:doc_version});
    Ok(())
}

pub fn leave_sub_state(state:&mut PersonaState,sub:&mut Option<SubPersonaSession>,store:&PersonaStore,apply:bool)->Result<serde_json::Value,CommandError>{
    let Some(session)=sub.clone() else {
        if apply{return Err(CommandError::new("NOT_IN_SUB_PERSONA"));}
        return Ok(serde_json::json!({"applied":false,"noop":true}));
    };
    let mut candidate=state.clone();switch_persona(&mut candidate,session.previous.clone())?;
    store.save(&candidate).map_err(|_|CommandError::new("IO_ERROR"))?;
    *state=candidate;*sub=None;
    Ok(serde_json::json!({"applied":apply,"doc_version_at_enter":session.doc_version_at_enter}))
}

struct Host {state:PersonaState,store:PersonaStore,revision:u64,sub_session:Option<SubPersonaSession>}
static HOST:OnceLock<Mutex<Host>>=OnceLock::new();
pub fn install(store:PersonaStore)->Result<(),CommandError>{
    let state=store.load();HOST.set(Mutex::new(Host{state,store,revision:1,sub_session:None})).map_err(|_|CommandError::new("CONFLICT"))?;
    if !dtool_core::commands::persona::register_action(shared_action){return Err(CommandError::new("CONFLICT"));}Ok(())
}
fn shared_action(command:&Command)->CommandResult{
    let mut host=HOST.get().ok_or_else(||CommandError::new("NOT_AVAILABLE"))?.lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?;
    if command.name=="ui.persona.set" {
        let id=command.params.get("id").and_then(|value|value.as_str()).ok_or_else(CommandError::invalid_params)?;
        let entry=PersonaEntry::parse(id)?;
        refuse_planned(&entry)?;
        if let PersonaEntry::Custom(ref name)=entry{
            if !host.state.custom_studios.iter().any(|studio|&studio.name==name){return Err(CommandError::not_found());}
        }
        refuse_excluded(&entry)?;
        let mut candidate=host.state.clone();switch_persona(&mut candidate,entry)?;
        host.store.save(&candidate).map_err(|_|CommandError::new("IO_ERROR"))?;host.state=candidate;host.revision+=1;
    } else if command.name=="ui.persona.list" {
        let mut items:Vec<serde_json::Value>=persona_catalog().iter().map(|def|serde_json::json!({"id":def.entry.key(),"name_key":def.name_key,"icon":def.icon,"status":persona_status(&def.entry)})).collect();
        for studio in &host.state.custom_studios{
            let icon = format!("persona.{}", PersonaEntry::Builtin(studio.base).key());
            items.push(serde_json::json!({"id":format!("custom:{}",studio.name),"name":studio.name,"name_key":"studio.custom","icon":icon,"status":"supported"}));
        }
        return Ok(serde_json::Value::Array(items));
    } else if matches!(command.name.as_str(),"ui.studio.create"|"ui.studio.update"|"ui.studio.rename"|"ui.studio.delete"|"ui.studio.import") {
        let mut candidate=host.state.clone();
        let reply=match command.name.as_str(){
            "ui.studio.create"=>{let (studio,skipped)=create_studio(&mut candidate,&command.params)?;serde_json::json!({"name":studio.name,"base":studio.base,"id":PersonaEntry::Custom(studio.name.clone()).key(),"skipped":skipped})}
            "ui.studio.update"=>{let skipped=update_studio(&mut candidate,&command.params)?;serde_json::json!({"name":command.params["name"],"skipped":skipped})}
            "ui.studio.rename"=>{rename_studio(&mut candidate,&command.params)?;serde_json::json!({"name":command.params["new_name"]})}
            "ui.studio.delete"=>{delete_studio(&mut candidate,&command.params)?;serde_json::json!({"deleted":command.params["name"]})}
            _=>{
                let path=command.params.get("path").and_then(|v|v.as_str()).ok_or_else(CommandError::invalid_params)?;
                let StudioFile { studio, tool_appearance } = read_studio(std::path::Path::new(path))?;
                studio_base(Some(&PersonaEntry::Builtin(studio.base).key()))?;
                let entry = PersonaEntry::Custom(studio.name.clone());
                if let Some(previous) = candidate.custom_studios.iter_mut().find(|item| item.name == studio.name) {
                    if command.params["replace_existing"].as_bool() == Some(false) {
                        return Err(CommandError {
                            code: "CONFLICT".into(),
                            message_key: "studio.manager.import_conflict".into(),
                        });
                    }
                    if previous.base != studio.base {
                        return Err(CommandError::new("CONFLICT"));
                    }
                    let layout = if candidate.active == entry {
                        Some(&mut candidate.live)
                    } else {
                        candidate.layouts.get_mut(&entry.key())
                    };
                    if let Some(layout) = layout {
                        apply_studio_changes(layout, previous, &studio)?;
                    }
                    *previous = studio.clone();
                } else {
                    candidate.custom_studios.push(studio.clone());
                }
                if let Some(appearance) = tool_appearance {
                    let layout = if candidate.active == entry {
                        &mut candidate.live
                    } else {
                        candidate.layouts.entry(entry.key()).or_insert_with(|| initial_studio(&studio))
                    };
                    appearance.apply(layout);
                }
                serde_json::json!({"name":studio.name,"base":studio.base})
            }
        };
        host.store.save(&candidate).map_err(|_|CommandError::new("IO_ERROR"))?;host.state=candidate;host.revision+=1;
        return Ok(reply);
    } else if command.name=="ui.studio.list" {
        return Ok(serde_json::to_value(&host.state.custom_studios).map_err(|_|CommandError::new("IO_ERROR"))?);
    } else if command.name=="ui.studio.export" {
        let name=command.params.get("name").and_then(|v|v.as_str()).ok_or_else(CommandError::invalid_params)?;
        let path=command.params.get("path").and_then(|v|v.as_str()).ok_or_else(CommandError::invalid_params)?;
        let studio=host.state.custom_studios.iter().find(|studio|studio.name==name).ok_or_else(CommandError::not_found)?;
        let entry = PersonaEntry::Custom(studio.name.clone());
        let layout = if host.state.active == entry {
            Some(&host.state.live)
        } else {
            host.state.layouts.get(&entry.key())
        };
        write_studio(studio, layout, std::path::Path::new(path))?;
        return Ok(serde_json::json!({"name":name,"path":path}));
    } else if command.name=="ui.persona.enter_sub" {
        let id=command.params.get("id").and_then(|value|value.as_str()).ok_or_else(CommandError::invalid_params)?;
        let doc_version=command.params.get("doc_version").and_then(|value|value.as_u64()).unwrap_or(0);
        let host=&mut *host;
        enter_sub_state(&mut host.state,&mut host.sub_session,&host.store,id,doc_version)?;
        host.revision+=1;
    } else if command.name=="ui.persona.leave_sub" {
        let apply=command.params.get("apply").and_then(|value|value.as_bool()).unwrap_or(false);
        let host=&mut *host;
        let was_in=host.sub_session.is_some();
        let reply=leave_sub_state(&mut host.state,&mut host.sub_session,&host.store,apply)?;
        if was_in{host.revision+=1;}
        return Ok(reply);
    } else if command.name!="ui.persona.query"{return Err(CommandError::not_implemented());}
    let base_id = match &host.state.active {
        PersonaEntry::Custom(name) => host.state.custom_studios.iter().find(|studio| &studio.name == name)
            .map(|studio| PersonaEntry::Builtin(studio.base).key()),
        _ => None,
    };
    Ok(serde_json::json!({"id":host.state.active.key(),"base_id":base_id,"layout":host.state.live,"revision":host.revision,"active_tool":host.state.live.tool.as_ref().map(|tool|tool.0),"in_sub_persona":host.sub_session.is_some()}))
}

pub fn live_dock()->Option<Layout>{
    let host=HOST.get()?.lock().ok()?;
    serde_json::from_value(host.state.live.dock.clone()).ok()
}

pub fn live_tools() -> Option<Vec<ToolId>> {
    let host = HOST.get()?.lock().ok()?;
    Some(host.state.live.tools.clone())
}

pub(crate) fn live_tool_appearance() -> (u8, Vec<ToolId>) {
    HOST.get().and_then(|host| host.lock().ok()).map(|host| {
        let layout = &host.state.live;
        (layout.tool_columns.unwrap_or(1), layout.standalone_tools.clone())
    }).unwrap_or_else(|| (1, Vec::new()))
}

pub(crate) fn reset_tools() -> Option<Vec<ToolId>> {
    let host = HOST.get()?.lock().ok()?;
    let base = match &host.state.active {
        PersonaEntry::Custom(name) => PersonaEntry::Builtin(host.state.custom_studios.iter().find(|studio| &studio.name == name)?.base),
        entry => entry.clone(),
    };
    persona_catalog().into_iter().find(|entry| entry.entry == base).map(|entry| entry.tools)
}

pub(crate) fn save_live_tools(tools: &[ToolId], columns: u8, standalone_tools: &[ToolId]) -> Result<(), CommandError> {
    let mut host = HOST.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?
        .lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
    let mut candidate = host.state.clone();
    candidate.live.tools = tools.to_vec();
    candidate.live.tool_columns = Some(columns.clamp(1, 4));
    candidate.live.standalone_tools = standalone_tools.to_vec();
    if let PersonaEntry::Custom(name) = &candidate.active
        && let Some(studio) = candidate.custom_studios.iter_mut().find(|studio| &studio.name == name)
    {
        studio.tools = tools.to_vec();
    }
    host.store.save(&candidate).map_err(|_| CommandError::new("IO_ERROR"))?;
    host.state = candidate;
    Ok(())
}

/// Persist an explicitly chosen dock layout without switching persona or tools.
pub fn save_live_dock(layout: &Layout) -> Result<(), CommandError> {
    let mut host = HOST.get().ok_or_else(||CommandError::new("NOT_AVAILABLE"))?.lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?;
    let mut candidate = host.state.clone();
    candidate.live.dock = serde_json::to_value(layout).map_err(|_|CommandError::invalid_params())?;
    candidate.live.panels = layout.panels.iter().filter(|p| p.visible).map(|p| p.id.clone()).collect();
    host.store.save(&candidate).map_err(|_|CommandError::new("IO_ERROR"))?;
    host.state = candidate;
    Ok(())
}

pub fn reconcile(layout:&mut Layout,session:&mut Session,tools:&mut Vec<ToolId>,toolbar:&mut Vec<String>,seen:&mut u64)->Result<(),CommandError>{
    let mut host=HOST.get().ok_or_else(||CommandError::new("NOT_AVAILABLE"))?.lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?;
    if *seen!=host.revision {
        *layout=serde_json::from_value(host.state.live.dock.clone()).map_err(|_|CommandError::invalid_params())?;
        *tools=host.state.live.tools.clone();*toolbar=host.state.live.toolbar.clone();
        if let Some(tool)=&host.state.live.tool{session.active_tool=tool.0.into();}*seen=host.revision;
    }else{
        host.state.live.dock=serde_json::to_value(&*layout).map_err(|_|CommandError::invalid_params())?;
        host.state.live.panels=crate::panel_ids::NAMED_PANELS.into_iter().chain(crate::panel_ids::remaining_panels()).filter(|panel|layout.is_panel_shown(panel)).collect();
        host.state.live.tool=tool(&session.active_tool);host.state.live.tools=tools.clone();host.state.live.toolbar=toolbar.clone();
    }
    Ok(())
}
