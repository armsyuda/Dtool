use std::borrow::Cow;

use lopdf::Stream;

use super::{Components, FunctionSamples, Limits, SampleSpan, interpolate, sampled};

const MAX_STREAM: usize = 16 * 1024;
const MAX_STACK: usize = 100;
const TOLERANCE: f64 = 0.25 / 255.0;

#[derive(Clone, Copy)]
enum Value {
    Int(i32),
    Real(f64),
    Bool(bool),
}

use Value::{Bool, Int, Real};

impl Value {
    fn number(self) -> Option<f64> {
        match self {
            Int(value) => Some(f64::from(value)),
            Real(value) => Some(value),
            Bool(_) => None,
        }
    }

    fn integer(self) -> Option<i32> {
        let Int(value) = self else {
            return None;
        };
        Some(value)
    }
}

enum Instruction<'a> {
    Literal(Value),
    Operator(&'a str),
    Branch(Vec<Self>, Vec<Self>),
}

fn tokens(data: &str) -> impl Iterator<Item = &str> {
    data.split(['\r', '\n']).flat_map(|line| {
        let code = line.split_once('%').map_or(line, |(code, _)| code);
        code.split_inclusive(['{', '}']).flat_map(|part| {
            let cut = part.len() - usize::from(part.ends_with(['{', '}']));
            let (word, brace) = part.split_at(cut);
            word.split(['\0', '\t', '\u{000c}', ' '])
                .filter(|word| !word.is_empty())
                .chain((!brace.is_empty()).then_some(brace))
        })
    })
}

fn parse<'a>(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
    budget: &mut usize,
    depth: usize,
) -> Option<Vec<Instruction<'a>>> {
    if depth > 32 {
        return None;
    }
    let mut result = Vec::new();
    loop {
        let token = tokens.next()?;
        if token == "}" {
            return Some(result);
        }
        *budget = budget.checked_sub(1)?;
        let instruction = match token {
            "{" => {
                let yes = parse(tokens, budget, depth + 1)?;
                let no = if tokens.peek() == Some(&"{") {
                    tokens.next();
                    let no = parse(tokens, budget, depth + 1)?;
                    if tokens.next()? != "ifelse" {
                        return None;
                    }
                    no
                } else {
                    if tokens.next()? != "if" {
                        return None;
                    }
                    Vec::new()
                };
                Instruction::Branch(yes, no)
            }
            "true" | "false" => Instruction::Literal(Bool(token == "true")),
            "abs" | "add" | "atan" | "ceiling" | "cos" | "cvi" | "cvr" | "div" | "exp"
            | "floor" | "idiv" | "ln" | "log" | "mod" | "mul" | "neg" | "round" | "sin"
            | "sqrt" | "sub" | "truncate" | "and" | "bitshift" | "eq" | "ge" | "gt" | "le"
            | "lt" | "ne" | "not" | "or" | "xor" | "copy" | "dup" | "exch" | "index" | "pop"
            | "roll" => Instruction::Operator(token),
            literal => {
                let unsigned = literal.strip_prefix(['+', '-']).unwrap_or(literal);
                if unsigned.is_empty()
                    || !unsigned
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'.')
                {
                    return None;
                }
                let value = if let Ok(value) = literal.parse::<i32>() {
                    Int(value)
                } else {
                    let value: f64 = literal.parse().ok()?;
                    if !value.is_finite() {
                        return None;
                    }
                    Real(value)
                };
                Instruction::Literal(value)
            }
        };
        result.push(instruction);
    }
}

fn run(program: &[Instruction<'_>], stack: &mut Vec<Value>, budget: &mut usize) -> Option<()> {
    for instruction in program {
        *budget = budget.checked_sub(1)?;
        match instruction {
            Instruction::Literal(value) => stack.push(*value),
            Instruction::Operator(operator) => apply(operator, stack)?,
            Instruction::Branch(yes, no) => {
                let Bool(test) = stack.pop()? else {
                    return None;
                };
                run(if test { yes } else { no }, stack, budget)?;
            }
        }
        if stack.len() > MAX_STACK {
            return None;
        }
    }
    Some(())
}

fn apply(operator: &str, stack: &mut Vec<Value>) -> Option<()> {
    let right = stack.pop()?;
    let value = match operator {
        "pop" => return Some(()),
        "dup" => {
            stack.push(right);
            right
        }
        "exch" => {
            let left = stack.pop()?;
            stack.push(right);
            left
        }
        "copy" | "index" => {
            let count = usize::try_from(right.integer()?).ok()?;
            if operator == "copy" {
                let from = stack.len().checked_sub(count)?;
                if stack.len() + count > MAX_STACK {
                    return None;
                }
                stack.extend_from_within(from..);
                return Some(());
            }
            stack[stack.len().checked_sub(count + 1)?]
        }
        "roll" => {
            let by = right.integer()?;
            let count = usize::try_from(stack.pop()?.integer()?).ok()?;
            let start = stack.len().checked_sub(count)?;
            if count > 0 {
                stack[start..].rotate_right(by.rem_euclid(count as i32) as usize);
            }
            return Some(());
        }
        "not" => match right {
            Bool(value) => Bool(!value),
            Int(value) => Int(!value),
            Real(_) => return None,
        },
        "abs" | "neg" | "ceiling" | "floor" | "round" | "truncate" | "sin" | "cos" | "sqrt"
        | "ln" | "log" | "cvr" | "cvi" => unary(operator, right)?,
        _ => binary(operator, stack.pop()?, right)?,
    };
    if matches!(value, Real(number) if !number.is_finite()) {
        return None;
    }
    stack.push(value);
    Some(())
}

fn unary(operator: &str, value: Value) -> Option<Value> {
    let number = value.number()?;
    Some(match (operator, value) {
        ("abs", Int(value)) => value.checked_abs().map_or(Real(number.abs()), Int),
        ("neg", Int(value)) => value.checked_neg().map_or(Real(-number), Int),
        ("ceiling" | "floor" | "round" | "truncate", Int(_)) => value,
        ("abs", _) => Real(number.abs()),
        ("neg", _) => Real(-number),
        ("ceiling", _) => Real(number.ceil()),
        ("floor", _) => Real(number.floor()),
        ("round", _) => {
            // PostScript rounds halfway values toward positive infinity.
            let floor = number.floor();
            Real(if number - floor >= 0.5 {
                floor + 1.0
            } else {
                floor
            })
        }
        ("truncate", _) => Real(number.trunc()),
        ("sin", _) => Real(number.to_radians().sin()),
        ("cos", _) => Real(number.to_radians().cos()),
        ("sqrt", _) => Real(number.sqrt()),
        ("ln", _) => Real(number.ln()),
        ("log", _) => Real(number.log10()),
        ("cvr", _) => Real(number),
        ("cvi", _) if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&number.trunc()) => {
            Int(number.trunc() as i32)
        }
        _ => return None,
    })
}

fn binary(operator: &str, left: Value, right: Value) -> Option<Value> {
    match (operator, left, right) {
        ("eq" | "ne", a, b) => {
            let equal = match (a, b) {
                (Bool(a), Bool(b)) => a == b,
                (Bool(_), _) | (_, Bool(_)) => false,
                _ => a.number()? == b.number()?,
            };
            return Some(Bool(equal == (operator == "eq")));
        }
        ("and", Bool(a), Bool(b)) => return Some(Bool(a & b)),
        ("or", Bool(a), Bool(b)) => return Some(Bool(a | b)),
        ("xor", Bool(a), Bool(b)) => return Some(Bool(a ^ b)),
        ("and", Int(a), Int(b)) => return Some(Int(a & b)),
        ("or", Int(a), Int(b)) => return Some(Int(a | b)),
        ("xor", Int(a), Int(b)) => return Some(Int(a ^ b)),
        ("idiv", Int(a), Int(b)) => return Some(Int(a.checked_div(b)?)),
        ("mod", Int(a), Int(b)) => return Some(Int(a.checked_rem(b)?)),
        ("bitshift", Int(a), Int(b)) => {
            let bits = a as u32;
            let shifted = if b < 0 {
                bits.checked_shr(b.unsigned_abs())
            } else {
                bits.checked_shl(b as u32)
            };
            return Some(Int(shifted.unwrap_or(0) as i32));
        }
        ("add" | "sub" | "mul", Int(a), Int(b)) => {
            let exact = match operator {
                "add" => a.checked_add(b),
                "sub" => a.checked_sub(b),
                _ => a.checked_mul(b),
            };
            if let Some(exact) = exact {
                return Some(Int(exact));
            }
        }
        _ => {}
    }
    let a = left.number()?;
    let b = right.number()?;
    Some(match operator {
        "add" => Real(a + b),
        "sub" => Real(a - b),
        "mul" => Real(a * b),
        "div" => Real(a / b),
        "atan" if a != 0.0 || b != 0.0 => Real(a.atan2(b).to_degrees().rem_euclid(360.0)),
        "exp" if a != 0.0 || b >= 0.0 => Real(a.powf(b)),
        "ge" => Bool(a >= b),
        "gt" => Bool(a > b),
        "le" => Bool(a <= b),
        "lt" => Bool(a < b),
        _ => return None,
    })
}

pub(super) fn append_samples(
    stream: &Stream,
    domain: [f64; 2],
    span: SampleSpan,
    range: Limits,
    output: &mut FunctionSamples,
) -> Option<()> {
    if !stream.dict.has(b"Range") {
        return None;
    }
    let data = if stream.dict.has(b"Filter") {
        Cow::Owned(stream.decompressed_content_with_limit(MAX_STREAM).ok()?)
    } else {
        Cow::Borrowed(stream.content.as_slice())
    };
    if data.len() > MAX_STREAM {
        return None;
    }
    // PDF comments may contain arbitrary bytes. Any invalid bytes outside them
    // become unrecognized tokens and are rejected by the parser.
    let text = String::from_utf8_lossy(&data);
    let mut tokens = tokens(&text).peekable();
    if tokens.next()? != "{" {
        return None;
    }
    let program = parse(&mut tokens, &mut 1024, 0)?;
    if tokens.next().is_some() {
        return None;
    }

    let mut stack = Vec::with_capacity(MAX_STACK + 1);
    let mut evaluate = |x: f64| {
        stack.clear();
        stack.push(Real(x.clamp(domain[0], domain[1])));
        run(&program, &mut stack, &mut output.remaining_calculator_steps)?;
        if stack.len() != output.components {
            return None;
        }
        let mut result = [0.0; 4];
        for (i, value) in stack.iter().enumerate() {
            result[i] = value.number()?.clamp(range[i][0], range[i][1]);
        }
        Some(result)
    };

    // Uniform spans and quarter-point probes catch curves whose midpoint lies on the chord.
    let mut cuts: Vec<_> = (1..64).map(|index| f64::from(index) / 64.0).collect();
    for boundary in domain {
        span.split_at(boundary, &mut cuts);
    }
    let first = evaluate(span.input[0])?;
    let mut vertices = vec![(span.offsets[0], first)];
    let mut start = first;
    for segment in span.segments(cuts) {
        let end = evaluate(segment.input[1])?;
        subdivide(segment, start, end, &mut evaluate, &mut vertices)?;
        start = end;
    }
    sampled::append_polyline(output, &vertices)
}

fn subdivide(
    span: SampleSpan,
    start: Components,
    end: Components,
    evaluate: &mut impl FnMut(f64) -> Option<Components>,
    vertices: &mut Vec<(f64, Components)>,
) -> Option<()> {
    if vertices.len() >= 8192 {
        return None;
    }
    // Keep both sides of a jump within the precision of gradient stop offsets.
    if span.offsets[1] - span.offsets[0] > 1e-9 && span.input[0] != span.input[1] {
        let input = span.input[0].midpoint(span.input[1]);
        let middle = evaluate(input)?;
        let mut split = false;
        for (fraction, value) in [
            (0.25, evaluate(interpolate(span.input, 0.25))?),
            (0.5, middle),
            (0.75, evaluate(interpolate(span.input, 0.75))?),
        ] {
            split |= value.iter().enumerate().any(|(i, value)| {
                (value - interpolate([start[i], end[i]], fraction)).abs() > TOLERANCE
            });
        }
        if split {
            let offset = span.offsets[0].midpoint(span.offsets[1]);
            subdivide(
                SampleSpan {
                    offsets: [span.offsets[0], offset],
                    input: [span.input[0], input],
                },
                start,
                middle,
                evaluate,
                vertices,
            )?;
            return subdivide(
                SampleSpan {
                    offsets: [offset, span.offsets[1]],
                    input: [input, span.input[1]],
                },
                middle,
                end,
                evaluate,
                vertices,
            );
        }
    }
    vertices.push((span.offsets[1], end));
    Some(())
}
