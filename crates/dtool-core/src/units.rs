use dtool_protocol::CommandError;

use crate::document::Units;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadoutUnit {
    #[default]
    Document,
    Px,
    Pt,
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl ReadoutUnit {
    pub fn is_document(&self) -> bool {
        *self == Self::Document
    }

    /// Suffix and conversion factor from document coordinates.
    pub fn from_document(self, units: Units, dpi: f32) -> (&'static str, f64) {
        let per_inch = |unit| f64::from(per_inch(unit, dpi));
        let (suffix, target) = match self {
            Self::Document => return (suffix(units), 1.0),
            Self::Px => ("px", per_inch(Units::Px)),
            Self::Pt => ("pt", per_inch(Units::Pt)),
            Self::Mm => ("mm", per_inch(Units::Mm)),
            Self::Cm => ("cm", per_inch(Units::Mm) / 10.0),
            Self::M => ("m", per_inch(Units::Mm) / 1000.0),
            Self::In => ("in", 1.0),
            Self::Ft => ("ft", 1.0 / 12.0),
        };
        (suffix, target / per_inch(units))
    }
}

pub fn suffix(units: Units) -> &'static str {
    match units {
        Units::Px => "px",
        Units::Pt => "pt",
        Units::Mm => "mm",
        Units::Cm => "cm",
        Units::In => "in",
    }
}

pub const PT_PER_INCH: f32 = 72.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitToken {
    Unit(Units),
    Pica,
    Meter,
}

fn safe_dpi(dpi: f32) -> f32 {
    if dpi.is_finite() && dpi > 0.0 { dpi } else { 72.0 }
}

pub fn length_error() -> CommandError {
    CommandError::invalid_params()
}

pub fn per_inch(unit: Units, dpi: f32) -> f32 {
    match unit {
        Units::Px => safe_dpi(dpi),
        Units::Pt => PT_PER_INCH,
        Units::Mm => 25.4,
        Units::Cm => 2.54,
        Units::In => 1.0,
    }
}

pub fn units_per_point(unit: Units, dpi: f32) -> f32 {
    per_inch(unit, dpi) / PT_PER_INCH
}

pub fn to_px(value: f32, unit: Units, dpi: f32) -> Option<f32> {
    let px = value / per_inch(unit, dpi) * safe_dpi(dpi);
    px.is_finite().then_some(px)
}

pub fn from_px(px: f32, unit: Units, dpi: f32) -> Option<f32> {
    let value = px / safe_dpi(dpi) * per_inch(unit, dpi);
    value.is_finite().then_some(value)
}

pub fn px_to_pt(px: f32, dpi: f32) -> f32 {
    px * PT_PER_INCH / safe_dpi(dpi)
}

pub fn pt_to_px(pt: f32, dpi: f32) -> f32 {
    pt * safe_dpi(dpi) / PT_PER_INCH
}

pub fn parse_unit(text: &str) -> Option<UnitToken> {
    let unit = match text.trim().to_lowercase().as_str() {
        "px" | "pixel" | "pixels" => Units::Px,
        "pt" | "point" | "points" => Units::Pt,
        "pc" | "pica" | "picas" => return Some(UnitToken::Pica),
        "m" | "meter" | "meters" | "metre" | "metres" => return Some(UnitToken::Meter),
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => Units::Mm,
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => Units::Cm,
        "in" | "inch" | "inches" | "\"" => Units::In,
        _ => return None,
    };
    Some(UnitToken::Unit(unit))
}

pub fn token_to_px(value: f32, token: UnitToken, dpi: f32) -> Option<f32> {
    match token {
        UnitToken::Unit(unit) => to_px(value, unit, dpi),
        UnitToken::Pica => to_px(value * 12.0, Units::Pt, dpi),
        UnitToken::Meter => to_px(value * 1_000.0, Units::Mm, dpi),
    }
}

#[derive(Debug, Clone, Copy)]
enum Quantity {
    Length(f64),
    Scalar(f64),
}

struct Parser {
    chars: Vec<char>,
    at: usize,
    default_unit: Units,
    dpi: f32,
    units: bool,
}

impl Parser {
    fn peek(&mut self) -> Option<char> {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
        self.chars.get(self.at).copied()
    }

    fn scalar_to_length(&self, value: f64) -> Result<f64, CommandError> {
        to_px(value as f32, self.default_unit, self.dpi).map(f64::from).ok_or_else(length_error)
    }

    fn as_length(&self, quantity: Quantity) -> Result<f64, CommandError> {
        match quantity {
            Quantity::Length(value) => Ok(value),
            Quantity::Scalar(value) => self.scalar_to_length(value),
        }
    }

    fn expr(&mut self) -> Result<Quantity, CommandError> {
        let mut left = self.term()?;
        while let Some(op) = self.peek().filter(|c| *c == '+' || *c == '-') {
            self.at += 1;
            let right = self.term()?;
            left = match (left, right) {
                (Quantity::Scalar(a), Quantity::Scalar(b)) => Quantity::Scalar(if op == '+' { a + b } else { a - b }),
                (a, b) => {
                    let (a, b) = (self.as_length(a)?, self.as_length(b)?);
                    Quantity::Length(if op == '+' { a + b } else { a - b })
                }
            };
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Quantity, CommandError> {
        let mut left = self.factor()?;
        while let Some(op) = self.peek().filter(|c| *c == '*' || *c == '/') {
            self.at += 1;
            let right = self.factor()?;
            left = match (op, left, right) {
                ('*', Quantity::Scalar(a), Quantity::Scalar(b)) => Quantity::Scalar(a * b),
                ('*', Quantity::Length(a), Quantity::Scalar(b)) | ('*', Quantity::Scalar(b), Quantity::Length(a)) => Quantity::Length(a * b),
                ('*', Quantity::Length(_), Quantity::Length(_)) => return Err(length_error()),
                (_, _, Quantity::Scalar(b)) | (_, _, Quantity::Length(b)) if b == 0.0 => return Err(length_error()),
                (_, Quantity::Scalar(a), Quantity::Scalar(b)) => Quantity::Scalar(a / b),
                (_, Quantity::Length(a), Quantity::Scalar(b)) => Quantity::Length(a / b),
                (_, Quantity::Length(a), Quantity::Length(b)) => Quantity::Scalar(a / b),
                (_, Quantity::Scalar(_), Quantity::Length(_)) => return Err(length_error()),
            };
        }
        Ok(left)
    }

    fn factor(&mut self) -> Result<Quantity, CommandError> {
        match self.peek() {
            Some('+') => {
                self.at += 1;
                self.factor()
            }
            Some('-') => {
                self.at += 1;
                Ok(match self.factor()? {
                    Quantity::Length(value) => Quantity::Length(-value),
                    Quantity::Scalar(value) => Quantity::Scalar(-value),
                })
            }
            Some('(') => {
                self.at += 1;
                let inner = self.expr()?;
                if self.peek() != Some(')') {
                    return Err(length_error());
                }
                self.at += 1;
                Ok(inner)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            _ => Err(length_error()),
        }
    }

    fn number(&mut self) -> Result<Quantity, CommandError> {
        let start = self.at;
        while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
            self.at += 1;
        }
        if self.chars.get(self.at).is_some_and(|c| *c == 'e' || *c == 'E')
            && self.chars.get(self.at + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
        {
            self.at += 2;
            while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let text: String = self.chars[start..self.at].iter().collect();
        let value: f64 = text.parse().map_err(|_| length_error())?;
        if !value.is_finite() || value.abs() > f32::MAX as f64 {
            return Err(length_error());
        }
        let unit_start = self.at;
        while self.chars.get(self.at).is_some_and(|c| c.is_alphabetic() || *c == '"') {
            self.at += 1;
            if self.chars[self.at - 1] == '"' {
                break;
            }
        }
        if unit_start == self.at {
            return Ok(Quantity::Scalar(value));
        }
        if !self.units {
            return Err(length_error());
        }
        let unit: String = self.chars[unit_start..self.at].iter().collect();
        let token = parse_unit(&unit).ok_or_else(length_error)?;
        token_to_px(value as f32, token, self.dpi).map(|px| Quantity::Length(px as f64)).ok_or_else(length_error)
    }
}

fn parse_with(text: &str, default_unit: Units, dpi: f32, units: bool) -> Result<Quantity, CommandError> {
    let mut parser = Parser { chars: text.chars().collect(), at: 0, default_unit, dpi, units };
    if parser.peek().is_none() {
        return Err(length_error());
    }
    let value = parser.expr()?;
    if parser.peek().is_some() {
        return Err(length_error());
    }
    Ok(value)
}

fn finite(value: f64) -> Result<f32, CommandError> {
    let value = value as f32;
    if value.is_finite() { Ok(value) } else { Err(length_error()) }
}

pub fn parse_length(text: &str, default_unit: Units, dpi: f32) -> Result<f32, CommandError> {
    match parse_with(text, default_unit, dpi, true)? {
        Quantity::Length(value) => finite(value),
        Quantity::Scalar(value) => to_px(finite(value)?, default_unit, dpi).ok_or_else(length_error),
    }
}

pub fn parse_length_value(value: &serde_json::Value, dpi: f32) -> Result<f32, CommandError> {
    match value {
        serde_json::Value::Number(number) => finite(number.as_f64().ok_or_else(length_error)?),
        serde_json::Value::String(text) => parse_length(text, Units::Px, dpi),
        _ => Err(length_error()),
    }
}

pub fn parse_number(text: &str) -> Result<f32, CommandError> {
    let trimmed = text.trim();
    let body = trimmed.strip_suffix('%').or_else(|| trimmed.strip_suffix('°')).unwrap_or(trimmed);
    match parse_with(body, Units::Px, 72.0, false)? {
        Quantity::Scalar(value) => finite(value),
        Quantity::Length(_) => Err(length_error()),
    }
}
