use dtool_core::document::Units;
use dtool_core::layer_style::raster::render_region;
use dtool_core::layer_style::{default_effect, BevelType, EffectKind, EffectParams};
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_core::{CanvasFlags, Document};
use dtool_core::commands::colour_pick::PickMode;
use dtool_render::pick_sampler::{pick, rasterize_with_effects};

#[test]
fn pick_and_effect_raster_follow_the_shared_bevel_path() {
    for kind in [BevelType::Inner, BevelType::Outer, BevelType::Emboss, BevelType::Pillow] {
        let mut effect = default_effect(EffectKind::BevelEmboss);
        effect.params = EffectParams::Light { radius: 4.0, depth: 1.0, soften: 0.0, azimuth: 135.0, elevation: 45.0, bevel: kind, highlight: Colour::new(255, 255, 255, 255), shadow: Colour::new(0, 0, 0, 255) };
        let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
        doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects: vec![effect], ..Object::default() });
        let flags = CanvasFlags::default();
        let bounds = [0.0, 0.0, 64.0, 64.0];
        assert_eq!(rasterize_with_effects(&doc, &flags, &[], bounds, 64, 64), render_region(&doc, &flags, &[], bounds, 64, 64), "{kind:?}");
        for x in [9.5f32, 10.5, 49.5, 50.5] {
            let p = render_region(&doc, &flags, &[], [x - 0.5, 29.5, 1.0, 1.0], 1, 1);
            let picked = pick(&doc, x, 30.0, PickMode::Point, None);
            if p[3] == 0 {
                continue;
            }
            let c = picked.unwrap_or_else(|| panic!("{kind:?} {x}"));
            assert_eq!([c.r, c.g, c.b], [p[0], p[1], p[2]], "{kind:?} {x}");
        }
    }
}
