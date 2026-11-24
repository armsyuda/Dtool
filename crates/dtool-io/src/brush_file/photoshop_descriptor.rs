use std::collections::HashMap;

use dtool_core::brush::{BrushDynamics, Controller, ControllerKind, PathScatter, ScatterAxis, ShapeJitter};

use super::Input;

pub(super) struct Preset {
    pub name: String,
    pub tip: PresetTip,
    pub size: Option<f32>,
    pub angle: Option<f32>,
    pub spacing: Option<f32>,
    pub dynamics: BrushDynamics,
}

pub(super) enum PresetTip {
    Sampled(String),
    Computed { hardness: f32, roundness: f32 },
}

enum Value<'a> {
    Object { class: &'a [u8], fields: HashMap<&'a [u8], Value<'a>> },
    List(Vec<Value<'a>>),
    Text(String),
    Unit([u8; 4], f32),
    Bool(bool),
    Long(i32),
    Double(f64),
    Other,
}

impl Value<'_> {
    fn get(&self, key: &[u8]) -> Option<&Self> {
        match self {
            Self::Object { fields, .. } => fields.get(key),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    fn unit(&self, key: &[u8; 4]) -> Option<f32> {
        match self {
            Self::Unit(unit, value) if unit == key && value.is_finite() => Some(*value),
            _ => None,
        }
    }
}

pub(super) fn presets(bytes: &[u8]) -> Option<(Vec<Preset>, usize)> {
    let mut input = Input(bytes);
    if input.be32()? != 16 {
        return None;
    }
    let root = object(&mut input, 0)?;
    let Value::List(brushes) = root.get(b"Brsh")? else {
        return None;
    };
    let presets: Vec<_> = brushes
        .iter()
        .filter_map(|brush| {
            let shape = brush.get(b"Brsh")?;
            let size = shape.get(b"Dmtr").and_then(|v| v.unit(b"#Pxl"));
            let tip = match shape {
                Value::Object { class: b"computedBrush", .. } => {
                    size.filter(|size| *size > 0.0)?;
                    PresetTip::Computed {
                        hardness: shape.get(b"Hrdn")?.unit(b"#Prc")?,
                        roundness: shape.get(b"Rndn")?.unit(b"#Prc")?,
                    }
                }
                _ => PresetTip::Sampled(shape.get(b"sampledData")?.text()?.to_owned()),
            };
            Some(Preset {
                name: brush
                    .get(b"Nm  ")
                    .or_else(|| shape.get(b"Nm  "))?
                    .text()?
                    .to_owned(),
                tip,
                size,
                angle: shape.get(b"Angl").and_then(|v| v.unit(b"#Ang")),
                spacing: shape.get(b"Spcn").and_then(|v| v.unit(b"#Prc")),
                dynamics: brush_dynamics(brush),
            })
        })
        .collect();
    let unsupported = brushes.len() - presets.len();
    Some((presets, unsupported))
}

fn brush_dynamics(brush: &Value<'_>) -> BrushDynamics {
    let mut dynamics = BrushDynamics::default();
    if matches!(brush.get(b"useTipDynamics"), Some(Value::Bool(true))) {
        let floor = brush.get(b"minimumDiameter").and_then(|v| v.unit(b"#Prc")).unwrap_or(0.0);
        dynamics.size = pressure_controller(brush.get(b"szVr"), floor / 100.0);
        if matches!(brush.get(b"szVr").and_then(|value| value.get(b"bVTy")), Some(Value::Long(3))) {
            dynamics.size = input_controller(ControllerKind::Tilt, floor / 100.0);
            dynamics.tilt_scale = Some(brush.get(b"tiltScale").and_then(|v| v.unit(b"#Prc"))
                .map_or(1.0, |value| (value / 100.0).clamp(0.0, 2.0)));
        }
        dynamics.size_jitter = shape_jitter(brush.get(b"szVr"), floor);
        let minimum = brush.get(b"minimumRoundness").and_then(|v| v.unit(b"#Prc")).unwrap_or(0.0);
        dynamics.roundness_jitter = shape_jitter(brush.get(b"roundnessDynamics"), minimum);
        if let Some(angle) = brush.get(b"angleDynamics") {
            dynamics.rotation_jitter = angle.get(b"jitter").and_then(|value| value.unit(b"#Prc"))
                .map_or(0.0, |value| (value / 100.0).clamp(0.0, 1.0));
            if matches!(angle.get(b"bVTy"), Some(Value::Long(6))) {
                dynamics.direction_rotation = 1.0;
            }
        }
    }
    if matches!(brush.get(b"usePaintDynamics"), Some(Value::Bool(true))) {
        dynamics.opacity = pressure_controller(brush.get(b"opVr"), 0.0);
        dynamics.flow = pressure_controller(brush.get(b"prVr"), 0.0);
    }
    if matches!(brush.get(b"useScatter"), Some(Value::Bool(true))) {
        let scatter = brush.get(b"scatterDynamics");
        let count = brush.get(b"countDynamics");
        let percent = |value: Option<&Value<'_>>, maximum: f32| value.and_then(|value| value.get(b"jitter"))
            .and_then(|value| value.unit(b"#Prc")).map_or(0.0, |value| (value / 100.0).clamp(0.0, maximum));
        let pressure = |value: Option<&Value<'_>>| matches!(value.and_then(|value| value.get(b"bVTy")), Some(Value::Long(2)));
        let copies = match brush.get(b"Cnt ") {
            Some(Value::Double(number)) if number.is_finite() => *number,
            Some(Value::Long(number)) => f64::from(*number),
            _ => 1.0,
        };
        dynamics.path_scatter = PathScatter {
            amount: percent(scatter, ScatterAxis::MAX_AMOUNT),
            both_axes: matches!(brush.get(b"bothAxes"), Some(Value::Bool(true))),
            count: copies.round().clamp(1.0, f64::from(PathScatter::MAX_COUNT)) as u8,
            count_jitter: percent(count, 1.0),
            pressure: pressure(scatter),
            count_pressure: pressure(count),
        };
    }
    dynamics
}

fn shape_jitter(variation: Option<&Value<'_>>, minimum: f32) -> ShapeJitter {
    let amount = variation.and_then(|value| value.get(b"jitter")).and_then(|value| value.unit(b"#Prc"))
        .map_or(0.0, |value| (value / 100.0).clamp(0.0, 1.0));
    if amount == 0.0 { return ShapeJitter::default(); }
    ShapeJitter { amount, minimum: (minimum / 100.0).clamp(0.0, 1.0) }
}

fn pressure_controller(variation: Option<&Value<'_>>, floor: f32) -> Controller {
    if let Some(variation) = variation
        && matches!(variation.get(b"bVTy"), Some(Value::Long(2)))
    {
        return input_controller(ControllerKind::Pressure, floor);
    }
    Controller::default()
}

fn input_controller(kind: ControllerKind, floor: f32) -> Controller {
    Controller {
        kind,
        curve: vec![[0.0, floor.clamp(0.0, 1.0)], [1.0, 1.0]],
        requires_pressure: kind == ControllerKind::Pressure,
        ..Controller::default()
    }
}

fn key<'a>(input: &mut Input<'a>) -> Option<&'a [u8]> {
    let length = input.be32()?;
    input.take(if length == 0 { 4 } else { length })
}

fn unicode(input: &mut Input<'_>) -> Option<String> {
    let count = input.be32()?;
    let bytes = input.take(count.checked_mul(2)?)?;
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .take(2048)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&units).ok()
}

fn object<'a>(input: &mut Input<'a>, depth: usize) -> Option<Value<'a>> {
    if depth > 12 {
        return None;
    }
    let name_length = input.be32()?.checked_mul(2)?;
    input.take(name_length)?;
    let class = key(input)?;
    let count = input.be32()?;
    if count > 4096 {
        return None;
    }
    let mut fields = HashMap::new();
    for _ in 0..count {
        let name = key(input)?;
        fields.insert(name, value(input, depth + 1)?);
    }
    Some(Value::Object { class, fields })
}

fn value<'a>(input: &mut Input<'a>, depth: usize) -> Option<Value<'a>> {
    if depth > 12 {
        return None;
    }
    let tag = input.take(4)?;
    Some(match tag {
        b"Objc" | b"GlbO" => object(input, depth)?,
        b"VlLs" => {
            let count = input.be32()?;
            if count > 4096 {
                return None;
            }
            Value::List(
                (0..count)
                    .map(|_| value(input, depth + 1))
                    .collect::<Option<_>>()?,
            )
        }
        b"TEXT" => Value::Text(unicode(input)?),
        b"UntF" => Value::Unit(input.array()?, f64::from_be_bytes(input.array()?) as f32),
        b"enum" => {
            key(input)?;
            key(input)?;
            Value::Other
        }
        b"long" => Value::Long(i32::from_be_bytes(input.array()?)),
        b"bool" => Value::Bool(input.array::<1>()?[0] != 0),
        b"doub" => Value::Double(f64::from_be_bytes(input.array()?)),
        b"comp" => {
            input.take(8)?;
            Value::Other
        }
        b"tdta" | b"alis" | b"Pth " => {
            let length = input.be32()?;
            input.take(length)?;
            Value::Other
        }
        _ => return None,
    })
}
