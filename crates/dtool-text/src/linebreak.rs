#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakClass {
    OP,
    CL,
    CP,
    EX,
    IS,
    NS,
    SY,
    AL,
    ID,
    NU,
    GL,
    WJ,
    ZW,
    BA,
    BB,
    HY,
    SP,
    CM,
    BK,
    CR,
    LF,
    NL,
    ZWJ,
    RI,
    XX,
}

pub fn break_class(ch: char) -> BreakClass {
    match ch {
        '\n' | '\u{000B}' | '\u{000C}' | '\u{2028}' | '\u{2029}' => BreakClass::BK,
        '\r' => BreakClass::CR,
        '\u{0085}' => BreakClass::NL,
        ' ' | '\t' | '\u{2000}'..='\u{200A}' | '\u{205F}' | '\u{3000}' => BreakClass::SP,
        '\u{200B}' => BreakClass::ZW,
        '\u{2060}' | '\u{FEFF}' => BreakClass::WJ,
        '\u{00A0}' | '\u{2011}' | '\u{202F}' | '\u{180E}' => BreakClass::GL,
        '\u{200D}' => BreakClass::ZWJ,
        '\u{00AD}' => BreakClass::BA,
        '(' | '[' | '{' | '\u{00AB}' | '\u{2018}' | '\u{201C}' | '\u{3008}' | '\u{300A}'
        | '\u{300C}' | '\u{300E}' | '\u{3010}' | '\u{3014}' | '\u{3016}' | '\u{3018}' | '\u{301A}'
        | '\u{301D}' | '\u{FF08}' | '\u{FF3B}' | '\u{FF5B}' | '\u{FF5F}' | '\u{FF62}' => BreakClass::OP,
        ')' | ']' | '}' | '\u{00BB}' | '\u{2019}' | '\u{201D}' | '\u{3009}' | '\u{300B}'
        | '\u{300D}' | '\u{300F}' | '\u{3011}' | '\u{3015}' | '\u{3017}' | '\u{3019}' | '\u{301B}'
        | '\u{301E}' | '\u{301F}' | '\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}' | '\u{FF61}'
        | '\u{FF64}' | '\u{FF09}' | '\u{FF3D}' | '\u{FF5D}' | '\u{FF60}' | '\u{FF63}' => {
            if matches!(ch, ')' | ']' | '\u{FF09}' | '\u{FF3D}') {
                BreakClass::CP
            } else {
                BreakClass::CL
            }
        }
        '!' | '?' | '\u{FF01}' | '\u{FF1F}' | '\u{203C}' | '\u{2047}' | '\u{2048}' | '\u{2049}' => {
            BreakClass::EX
        }
        '.' | ',' | ':' | ';' | '\u{FF1A}' => BreakClass::IS,
        '\u{30FC}' | '\u{301C}' | '\u{3030}' | '\u{FF70}' | '\u{30A0}' | '\u{2010}' | '\u{2013}'
        | '\u{309B}' | '\u{309C}' | '\u{309D}' | '\u{309E}' | '\u{30FD}' | '\u{30FE}' | '\u{3005}'
        | '\u{303B}' | '\u{FF1B}' | '\u{3041}' | '\u{3043}' | '\u{3045}' | '\u{3047}' | '\u{3049}'
        | '\u{3063}' | '\u{3083}' | '\u{3085}' | '\u{3087}' | '\u{308E}' | '\u{3095}' | '\u{3096}'
        | '\u{30A1}' | '\u{30A3}' | '\u{30A5}' | '\u{30A7}' | '\u{30A9}' | '\u{30C3}' | '\u{30E3}'
        | '\u{30E5}' | '\u{30E7}' | '\u{30EE}' | '\u{30F5}' | '\u{30F6}' | '\u{31F0}'..='\u{31FF}' => {
            BreakClass::NS
        }
        '/' | '\u{FF0F}' => BreakClass::SY,
        '-' => BreakClass::HY,
        _ => {
            if is_hangul_syllable(ch) || is_cjk_ideograph(ch) || is_cjk_symbol(ch) {
                BreakClass::ID
            } else if is_hangul_jamo(ch) {
                BreakClass::AL
            } else if ch.is_ascii_digit() || is_numeric(ch) {
                BreakClass::NU
            } else if ch.is_alphabetic() {
                BreakClass::AL
            } else if unic_combining(ch) {
                BreakClass::CM
            } else {
                BreakClass::AL
            }
        }
    }
}

fn is_hangul_syllable(ch: char) -> bool {
    ch >= '\u{AC00}' && ch <= '\u{D7A3}'
}

fn is_hangul_jamo(ch: char) -> bool {
    (ch >= '\u{1100}' && ch <= '\u{11FF}')
        || (ch >= '\u{A960}' && ch <= '\u{A97C}')
        || (ch >= '\u{D7B0}' && ch <= '\u{D7C6}')
        || (ch >= '\u{D7CB}' && ch <= '\u{D7FB}')
}

fn is_cjk_ideograph(ch: char) -> bool {
    (ch >= '\u{4E00}' && ch <= '\u{9FFF}')
        || (ch >= '\u{3400}' && ch <= '\u{4DBF}')
        || (ch >= '\u{F900}' && ch <= '\u{FAFF}')
        || (ch >= '\u{20000}' && ch <= '\u{2A6DF}')
        || (ch >= '\u{2A700}' && ch <= '\u{2B73F}')
        || (ch >= '\u{2B740}' && ch <= '\u{2B81F}')
        || (ch >= '\u{2B820}' && ch <= '\u{2CEAF}')
        || (ch >= '\u{30000}' && ch <= '\u{323AF}')
        || (ch >= '\u{3130}' && ch <= '\u{318F}')
}

fn is_cjk_symbol(ch: char) -> bool {
    (ch >= '\u{3040}' && ch <= '\u{309F}')
        || (ch >= '\u{30A1}' && ch <= '\u{30FB}')
        || (ch >= '\u{31F0}' && ch <= '\u{31FF}')
        || (ch >= '\u{FF66}' && ch <= '\u{FF9D}')
}

fn is_numeric(ch: char) -> bool {
    ch.is_ascii_digit() || (ch >= '\u{FF10}' && ch <= '\u{FF19}')
}

fn unic_combining(ch: char) -> bool {
    (ch >= '\u{0300}' && ch <= '\u{036F}')
        || (ch >= '\u{20D0}' && ch <= '\u{20FF}')
        || (ch >= '\u{FE00}' && ch <= '\u{FE0F}')
}

pub fn can_start_line(ch: char) -> bool {
    !matches!(
        break_class(ch),
        BreakClass::CL | BreakClass::CP | BreakClass::EX | BreakClass::IS | BreakClass::NS | BreakClass::SY
    )
}

pub fn can_end_line(ch: char) -> bool {
    !matches!(break_class(ch), BreakClass::OP)
}

pub fn can_break_between(before: char, after: char) -> bool {
    let a = break_class(before);
    let b = break_class(after);
    if matches!(a, BreakClass::BK | BreakClass::LF | BreakClass::NL) {
        return true;
    }
    if a == BreakClass::CR && b == BreakClass::LF {
        return false;
    }
    if a == BreakClass::CR {
        return true;
    }
    if matches!(b, BreakClass::BK | BreakClass::CR | BreakClass::LF | BreakClass::NL) {
        return false;
    }
    if b == BreakClass::CM || b == BreakClass::ZWJ {
        return false;
    }
    if a == BreakClass::ZWJ {
        return false;
    }
    if a == BreakClass::WJ || b == BreakClass::WJ {
        return false;
    }
    if a == BreakClass::GL || b == BreakClass::GL {
        return false;
    }
    if a == BreakClass::ZW {
        return b != BreakClass::SP && b != BreakClass::ZW;
    }
    if a == BreakClass::SP && b == BreakClass::ZW {
        return false;
    }
    if a == BreakClass::OP {
        return false;
    }
    if matches!(
        b,
        BreakClass::CL | BreakClass::CP | BreakClass::EX | BreakClass::IS | BreakClass::NS | BreakClass::SY
    ) {
        return false;
    }
    if matches!(a, BreakClass::CL | BreakClass::CP) && b == BreakClass::NS {
        return false;
    }
    if b == BreakClass::ZW {
        return false;
    }
    if a == BreakClass::SP {
        return true;
    }
    if b == BreakClass::SP {
        return false;
    }
    if a == BreakClass::HY && b == BreakClass::NU {
        return false;
    }
    if a == BreakClass::IS && b == BreakClass::NU {
        return false;
    }
    if a == BreakClass::SY && b == BreakClass::NU {
        return false;
    }
    if a == BreakClass::BB || b == BreakClass::BA || b == BreakClass::HY {
        return false;
    }
    if a == BreakClass::BA || a == BreakClass::HY {
        return true;
    }
    if a == BreakClass::AL && b == BreakClass::AL {
        return false;
    }
    if a == BreakClass::AL && b == BreakClass::NU {
        return false;
    }
    if a == BreakClass::NU && b == BreakClass::AL {
        return false;
    }
    if a == BreakClass::NU && b == BreakClass::NU {
        return false;
    }
    if a == BreakClass::RI && b == BreakClass::RI {
        return false;
    }
    true
}

pub fn can_break_before_char(before: Option<char>, after: char) -> bool {
    match before {
        Some(prev) => can_break_between(prev, after),
        None => can_start_line(after),
    }
}
