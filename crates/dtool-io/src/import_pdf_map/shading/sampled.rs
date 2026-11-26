use std::borrow::Cow;

use lopdf::Stream;

use super::{Components, FunctionSamples, Limits, PdfDocument, SampleSpan, deref, interpolate, interval, numbers};

pub(super) fn append_samples(
    pdf: &PdfDocument,
    stream: &Stream,
    domain: [f64; 2],
    span: SampleSpan,
    range: Limits,
    output: &mut FunctionSamples,
) -> Option<()> {
    let dict = &stream.dict;
    let read_int = |key: &[u8]| deref(pdf, dict.get(key).ok()?).as_i64().ok();
    let [size] = deref(pdf, dict.get(b"Size").ok()?).as_array().ok()?.as_slice() else { return None; };
    let size = usize::try_from(deref(pdf, size).as_i64().ok()?).ok()?;
    if !(1..=65_536).contains(&size) { return None; }
    let order = if dict.has(b"Order") { read_int(b"Order")? } else { 1 };
    if order != 1 && !(order == 3 && size < 4) { return None; }
    let bits = usize::try_from(read_int(b"BitsPerSample")?).ok()?;
    if !matches!(bits, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32) { return None; }
    let high_index = (size - 1) as f64;
    let encode = if dict.has(b"Encode") { interval(pdf, dict, b"Encode")? } else { [0.0, high_index] };
    let decode = dict.get(b"Decode").or_else(|_| dict.get(b"Range")).ok()?;
    let decode = numbers(pdf, decode)?;
    if !dict.has(b"Range") || decode.len() != output.components * 2 { return None; }
    let value_count = size * output.components;
    output.remaining_sample_values = output.remaining_sample_values.checked_sub(value_count)?;
    let byte_count = (value_count * bits).div_ceil(8);
    let data = if dict.has(b"Filter") {
        Cow::Owned(stream.decompressed_content_with_limit(byte_count.max(4096)).ok()?)
    } else {
        Cow::Borrowed(stream.content.as_slice())
    };
    if data.len() != byte_count { return None; }

    let maximum = (1u64 << bits) - 1;
    let table: Vec<Components> = (0..size).map(|entry| {
        let mut values = [0.0; 4];
        for channel in 0..output.components {
            let start = (entry * output.components + channel) * bits;
            let end = start + bits;
            let word = data[start / 8..end.div_ceil(8)].iter().fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
            let sample = (word >> (end.next_multiple_of(8) - end)) & maximum;
            values[channel] = interpolate([decode[channel * 2], decode[channel * 2 + 1]], sample as f64 / maximum as f64);
        }
        values
    }).collect();
    let lookup = |input: f64| {
        let index = input.clamp(0.0, high_index);
        let left = index.floor() as usize;
        let right = (left + 1).min(size - 1);
        std::array::from_fn(|i| interpolate([table[left][i], table[right][i]], index - left as f64))
    };
    let clamp_colour = |values: Components| {
        std::array::from_fn(|i| values[i].clamp(range[i][0], range[i][1]))
    };

    let mut domain_cuts = Vec::new();
    for edge in domain { span.split_at(edge, &mut domain_cuts); }
    let mut vertices = vec![(span.offsets[0], clamp_colour(lookup(encode_input(span.input[0], domain, encode))))];
    for section in span.segments(domain_cuts) {
        let encoded = SampleSpan { input: section.input.map(|x| encode_input(x, domain, encode)), ..section };
        let mut knots = Vec::new();
        let low = encoded.input[0].min(encoded.input[1]).max(0.0).ceil() as usize;
        let high = encoded.input[0].max(encoded.input[1]).min(high_index).floor() as usize;
        for index in low..=high { encoded.split_at(index as f64, &mut knots); }
        for band in encoded.segments(knots) {
            let [first, last] = band.input.map(lookup);
            let mut fractions = vec![1.0];
            for channel in 0..output.components {
                for bound in range[channel] {
                    let t = (bound - first[channel]) / (last[channel] - first[channel]);
                    if t > 0.0 && t < 1.0 { fractions.push(t); }
                }
            }
            fractions.sort_by(f64::total_cmp);
            fractions.dedup();
            for t in fractions {
                let colour = std::array::from_fn(|i| interpolate([first[i], last[i]], t));
                vertices.push((interpolate(band.offsets, t), clamp_colour(colour)));
            }
        }
    }
    append_polyline(output, &vertices)
}

fn encode_input(input: f64, [low, high]: [f64; 2], encode: [f64; 2]) -> f64 {
    interpolate(encode, ((input - low) / (high - low)).clamp(0.0, 1.0))
}

pub(super) fn append_polyline(output: &mut FunctionSamples, vertices: &[(f64, Components)]) -> Option<()> {
    // Bound the component error at every sampled vertex. The error between vertices is linear.
    const TOLERANCE: f64 = 0.25 / 255.0;
    const UNBOUNDED: Limits = [[f64::NEG_INFINITY, f64::INFINITY]; 4];
    let mut anchor = vertices[0];
    let mut previous = anchor;
    let mut slopes = UNBOUNDED;
    output.push(anchor.0, anchor.1)?;
    for &vertex in &vertices[1..] {
        if vertex.0 <= previous.0 { continue; }
        let span = vertex.0 - anchor.0;
        let fits = (0..output.components).all(|i| {
            let slope = (vertex.1[i] - anchor.1[i]) / span;
            (slopes[i][0]..=slopes[i][1]).contains(&slope)
        });
        if !fits {
            output.push(previous.0, previous.1)?;
            anchor = previous;
            slopes = UNBOUNDED;
        }
        let span = vertex.0 - anchor.0;
        for (i, limit) in slopes.iter_mut().enumerate().take(output.components) {
            let delta = vertex.1[i] - anchor.1[i];
            limit[0] = limit[0].max((delta - TOLERANCE) / span);
            limit[1] = limit[1].min((delta + TOLERANCE) / span);
        }
        previous = vertex;
    }
    output.push(previous.0, previous.1)
}
