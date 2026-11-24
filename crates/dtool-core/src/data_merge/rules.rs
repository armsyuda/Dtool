use std::cmp::Ordering;
use crate::CommandError;
use super::{Clause, DataSource, FilteredRecords, MergeRule, RowWarning, RuleAction};

struct Ctx<'a> {
    fields: &'a [String],
    case_sensitive: bool,
    numbers_as_text: bool,
}

#[derive(Clone, Debug)]
enum HitKind {
    Whole,
    Text(String),
    Begin(String),
    End(String),
    Pattern(String),
}

#[derive(Clone, Debug)]
struct Hit {
    field: usize,
    kind: HitKind,
}

pub fn apply_rules(src: &DataSource) -> Result<FilteredRecords, CommandError> {
    let ctx = Ctx {
        fields: &src.fields,
        case_sensitive: src.case_sensitive,
        numbers_as_text: src.numbers_as_text,
    };
    for rule in &src.rules {
        check_rule(&ctx, rule)?;
    }
    let mut out = FilteredRecords::default();
    'records: for (index, record) in src.records.iter().enumerate() {
        let mut row = record.clone();
        for (rule_index, rule) in src.rules.iter().enumerate() {
            let mut hits = Vec::new();
            if !rule_matches(&ctx, rule, &row, &mut hits)? {
                continue;
            }
            match &rule.action {
                RuleAction::Skip => continue 'records,
                RuleAction::Warn => out.warnings.push(rule_warning(index, rule_index)),
                RuleAction::WarnSkip => {
                    out.warnings.push(rule_warning(index, rule_index));
                    continue 'records;
                }
                RuleAction::ErrorStop => return Err(CommandError::new("MERGE_STOPPED")),
                action => transform(&ctx, action, &mut row, &hits)?,
            }
        }
        out.records.push(row);
        out.original_indices.push(index);
    }
    Ok(out)
}

pub fn add_field(src: &mut DataSource, name: &str, value: &str) -> Result<(), CommandError> {
    if name.is_empty() || src.fields.iter().any(|f| f == name) {
        return Err(CommandError::new("MERGE_BAD_FIELD"));
    }
    let width = src.fields.len();
    src.fields.push(name.to_string());
    for record in &mut src.records {
        if record.len() < width {
            record.resize(width, String::new());
        }
        record.insert(width, value.to_string());
    }
    Ok(())
}

pub fn discard_field(src: &mut DataSource, name: &str) -> Result<(), CommandError> {
    let index = src
        .fields
        .iter()
        .position(|f| f == name)
        .ok_or_else(|| CommandError::new("MERGE_BAD_FIELD"))?;
    src.fields.remove(index);
    for record in &mut src.records {
        if index < record.len() {
            record.remove(index);
        }
    }
    Ok(())
}

fn rule_warning(index: usize, rule_index: usize) -> RowWarning {
    RowWarning {
        row_index: index,
        message: format!("Rule {} matched record {}", rule_index + 1, index + 1),
    }
}

fn check_rule(ctx: &Ctx, rule: &MergeRule) -> Result<(), CommandError> {
    for clause in &rule.clauses {
        check_clause(ctx, clause)?;
    }
    if let RuleAction::RegexReplace(pattern, _) = &rule.action {
        Pattern::new(pattern, !ctx.case_sensitive)?;
    }
    Ok(())
}

fn check_clause(ctx: &Ctx, clause: &Clause) -> Result<(), CommandError> {
    match clause {
        Clause::And(list) | Clause::Or(list) | Clause::Nor(list) => {
            for inner in list {
                check_clause(ctx, inner)?;
            }
            Ok(())
        }
        Clause::FieldRegex(field, pattern) => {
            field_index(ctx, field)?;
            Pattern::new(pattern, !ctx.case_sensitive).map(|_| ())
        }
        Clause::FieldEquals(field, _)
        | Clause::FieldContains(field, _)
        | Clause::FieldNotEquals(field, _)
        | Clause::FieldBeginsWith(field, _)
        | Clause::FieldEndsWith(field, _)
        | Clause::FieldLessThan(field, _)
        | Clause::FieldLessOrEqual(field, _)
        | Clause::FieldGreaterThan(field, _)
        | Clause::FieldGreaterOrEqual(field, _) => field_index(ctx, field).map(|_| ()),
    }
}

fn field_index(ctx: &Ctx, name: &str) -> Result<usize, CommandError> {
    if let Some(index) = ctx.fields.iter().position(|f| f == name) {
        return Ok(index);
    }
    if !ctx.case_sensitive {
        let lower = name.to_lowercase();
        if let Some(index) = ctx.fields.iter().position(|f| f.to_lowercase() == lower) {
            return Ok(index);
        }
    }
    Err(CommandError::new("MERGE_BAD_FIELD"))
}

fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("")
}

fn rule_matches(ctx: &Ctx, rule: &MergeRule, row: &[String], hits: &mut Vec<Hit>) -> Result<bool, CommandError> {
    if rule.clauses.is_empty() {
        return Ok(false);
    }
    let mut local = Vec::new();
    let mut all = true;
    let mut any = false;
    for clause in &rule.clauses {
        let mut found = Vec::new();
        if eval(ctx, clause, row, &mut found)? {
            any = true;
            local.extend(found);
        } else {
            all = false;
        }
    }
    let matched = if rule.match_all { all } else { any };
    if matched {
        hits.extend(local);
    }
    Ok(matched)
}

fn eval(ctx: &Ctx, clause: &Clause, row: &[String], hits: &mut Vec<Hit>) -> Result<bool, CommandError> {
    let cs = ctx.case_sensitive;
    let (field, kind, ok) = match clause {
        Clause::And(list) => {
            let mut local = Vec::new();
            for inner in list {
                let mut found = Vec::new();
                if !eval(ctx, inner, row, &mut found)? {
                    return Ok(false);
                }
                local.extend(found);
            }
            hits.extend(local);
            return Ok(true);
        }
        Clause::Or(list) => {
            let mut any = false;
            for inner in list {
                let mut found = Vec::new();
                if eval(ctx, inner, row, &mut found)? {
                    any = true;
                    hits.extend(found);
                }
            }
            return Ok(any);
        }
        Clause::Nor(list) => {
            for inner in list {
                let mut found = Vec::new();
                if eval(ctx, inner, row, &mut found)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Clause::FieldEquals(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, text_eq(cell(row, i), v, cs))
        }
        Clause::FieldNotEquals(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, !text_eq(cell(row, i), v, cs))
        }
        Clause::FieldContains(f, v) => {
            let i = field_index(ctx, f)?;
            let hay: Vec<char> = cell(row, i).chars().collect();
            let needle: Vec<char> = v.chars().collect();
            let ok = needle.is_empty() || find_text(&hay, &needle, 0, cs).is_some();
            (i, HitKind::Text(v.clone()), ok)
        }
        Clause::FieldBeginsWith(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Begin(v.clone()), begins(cell(row, i), v, cs))
        }
        Clause::FieldEndsWith(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::End(v.clone()), ends(cell(row, i), v, cs))
        }
        Clause::FieldRegex(f, p) => {
            let i = field_index(ctx, f)?;
            let ok = Pattern::new(p, !cs)?.is_match(cell(row, i));
            (i, HitKind::Pattern(p.clone()), ok)
        }
        Clause::FieldLessThan(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, compare(ctx, cell(row, i), v) == Ordering::Less)
        }
        Clause::FieldLessOrEqual(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, compare(ctx, cell(row, i), v) != Ordering::Greater)
        }
        Clause::FieldGreaterThan(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, compare(ctx, cell(row, i), v) == Ordering::Greater)
        }
        Clause::FieldGreaterOrEqual(f, v) => {
            let i = field_index(ctx, f)?;
            (i, HitKind::Whole, compare(ctx, cell(row, i), v) != Ordering::Less)
        }
    };
    if ok {
        hits.push(Hit { field, kind });
    }
    Ok(ok)
}

fn compare(ctx: &Ctx, a: &str, b: &str) -> Ordering {
    if !ctx.numbers_as_text
        && let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
    {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    if ctx.case_sensitive {
        a.cmp(b)
    } else {
        a.to_lowercase().cmp(&b.to_lowercase())
    }
}

fn char_eq(a: char, b: char, cs: bool) -> bool {
    a == b || (!cs && a.to_lowercase().eq(b.to_lowercase()))
}

fn chars_eq(a: &[char], b: &[char], cs: bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| char_eq(*x, *y, cs))
}

fn text_eq(a: &str, b: &str, cs: bool) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    chars_eq(&a, &b, cs)
}

fn begins(value: &str, prefix: &str, cs: bool) -> bool {
    let v: Vec<char> = value.chars().collect();
    let p: Vec<char> = prefix.chars().collect();
    p.len() <= v.len() && chars_eq(&v[..p.len()], &p, cs)
}

fn ends(value: &str, suffix: &str, cs: bool) -> bool {
    let v: Vec<char> = value.chars().collect();
    let s: Vec<char> = suffix.chars().collect();
    s.len() <= v.len() && chars_eq(&v[v.len() - s.len()..], &s, cs)
}

fn find_text(hay: &[char], needle: &[char], from: usize, cs: bool) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| chars_eq(&hay[i..i + needle.len()], needle, cs))
}

fn replace_text(value: &str, needle: &str, rep: &str, cs: bool) -> String {
    let hay: Vec<char> = value.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    while let Some(i) = find_text(&hay, &needle, pos, cs) {
        out.extend(&hay[pos..i]);
        out.push_str(rep);
        pos = i + needle.len();
    }
    out.extend(&hay[pos..]);
    out
}

fn slot(row: &mut Vec<String>, index: usize) -> &mut String {
    if row.len() <= index {
        row.resize(index + 1, String::new());
    }
    &mut row[index]
}

fn transform(ctx: &Ctx, action: &RuleAction, row: &mut Vec<String>, hits: &[Hit]) -> Result<(), CommandError> {
    let cs = ctx.case_sensitive;
    let mut targets: Vec<usize> = Vec::new();
    for hit in hits {
        if !targets.contains(&hit.field) {
            targets.push(hit.field);
        }
    }
    match action {
        RuleAction::Prefix(text) => {
            for &i in &targets {
                let value = slot(row, i);
                *value = format!("{}{}", text, value);
            }
        }
        RuleAction::Postfix(text) => {
            for &i in &targets {
                slot(row, i).push_str(text);
            }
        }
        RuleAction::ReplaceAll(text) => {
            for &i in &targets {
                *slot(row, i) = text.clone();
            }
        }
        RuleAction::ReplaceMatched(text) => {
            for hit in hits {
                let value = slot(row, hit.field);
                let next = match &hit.kind {
                    HitKind::Whole => text.clone(),
                    HitKind::Text(needle) => replace_text(value, needle, text, cs),
                    HitKind::Begin(prefix) => {
                        if begins(value, prefix, cs) {
                            let rest: String = value.chars().skip(prefix.chars().count()).collect();
                            format!("{}{}", text, rest)
                        } else {
                            value.clone()
                        }
                    }
                    HitKind::End(suffix) => {
                        if ends(value, suffix, cs) {
                            let keep = value.chars().count() - suffix.chars().count();
                            let head: String = value.chars().take(keep).collect();
                            format!("{}{}", head, text)
                        } else {
                            value.clone()
                        }
                    }
                    HitKind::Pattern(pattern) => Pattern::new(pattern, !cs)?.replace_all(value, text, false),
                };
                *value = next;
            }
        }
        RuleAction::RegexReplace(pattern, rep) => {
            let compiled = Pattern::new(pattern, !cs)?;
            for &i in &targets {
                let value = slot(row, i);
                *value = compiled.replace_all(value, rep, true);
            }
        }
        RuleAction::Skip | RuleAction::Warn | RuleAction::WarnSkip | RuleAction::ErrorStop => {}
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

#[derive(Clone, Debug)]
enum Node {
    Literal(char),
    Any,
    Class(Vec<ClassItem>, bool),
    Start,
    End,
    Group(Vec<Vec<Node>>, Option<usize>),
    Repeat(Box<Node>, usize, Option<usize>, bool),
}

#[derive(Clone, Copy)]
enum Frame<'a> {
    Seq(&'a [Node]),
    Close(usize, usize),
    Again(&'a Node, usize, Option<usize>, bool, usize, usize),
}

struct Cont<'a> {
    frame: Frame<'a>,
    next: Option<&'a Cont<'a>>,
}

type Caps = Vec<Option<(usize, usize)>>;

struct Parser {
    chars: Vec<char>,
    pos: usize,
    groups: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn take(&mut self) -> Result<char, CommandError> {
        let c = self.peek().ok_or_else(CommandError::invalid_params)?;
        self.pos += 1;
        Ok(c)
    }

    fn alts(&mut self) -> Result<Vec<Vec<Node>>, CommandError> {
        let mut alts = vec![self.seq()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            alts.push(self.seq()?);
        }
        Ok(alts)
    }

    fn seq(&mut self) -> Result<Vec<Node>, CommandError> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            nodes.push(self.quantify(atom)?);
        }
        Ok(nodes)
    }

    fn atom(&mut self) -> Result<Node, CommandError> {
        let c = self.take()?;
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '(' => {
                let cap = if self.peek() == Some('?') && self.chars.get(self.pos + 1) == Some(&':') {
                    self.pos += 2;
                    None
                } else {
                    self.groups += 1;
                    Some(self.groups)
                };
                let alts = self.alts()?;
                if self.take()? != ')' {
                    return Err(CommandError::invalid_params());
                }
                Node::Group(alts, cap)
            }
            '[' => self.class()?,
            '\\' => {
                let e = self.take()?;
                match class_escape(e) {
                    Some(item) => Node::Class(vec![item], false),
                    None => Node::Literal(literal_escape(e)),
                }
            }
            '*' | '+' | '?' => return Err(CommandError::invalid_params()),
            other => Node::Literal(other),
        })
    }

    fn class(&mut self) -> Result<Node, CommandError> {
        let negated = self.peek() == Some('^');
        if negated {
            self.pos += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let c = self.take()?;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' {
                let e = self.take()?;
                if let Some(item) = class_escape(e) {
                    items.push(item);
                    continue;
                }
                literal_escape(e)
            } else {
                c
            };
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|n| *n != ']') {
                self.pos += 1;
                let mut hi = self.take()?;
                if hi == '\\' {
                    hi = literal_escape(self.take()?);
                }
                if hi < lo {
                    return Err(CommandError::invalid_params());
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Node::Class(items, negated))
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        self.chars[start..self.pos].iter().collect::<String>().parse().ok()
    }

    fn braces(&mut self) -> Option<(usize, Option<usize>)> {
        let save = self.pos;
        self.pos += 1;
        let parsed = self.number().and_then(|min| {
            let max = if self.peek() == Some(',') {
                self.pos += 1;
                self.number()
            } else {
                Some(min)
            };
            if self.peek() == Some('}') {
                self.pos += 1;
                Some((min, max))
            } else {
                None
            }
        });
        if parsed.is_none() {
            self.pos = save;
        }
        parsed
    }

    fn quantify(&mut self, atom: Node) -> Result<Node, CommandError> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('{') => match self.braces() {
                Some(range) => range,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        if max.is_some_and(|m| m < min) {
            return Err(CommandError::invalid_params());
        }
        let lazy = self.peek() == Some('?');
        if lazy {
            self.pos += 1;
        }
        Ok(Node::Repeat(Box::new(atom), min, max, lazy))
    }
}

fn class_escape(c: char) -> Option<ClassItem> {
    match c {
        'd' => Some(ClassItem::Digit(false)),
        'D' => Some(ClassItem::Digit(true)),
        'w' => Some(ClassItem::Word(false)),
        'W' => Some(ClassItem::Word(true)),
        's' => Some(ClassItem::Space(false)),
        'S' => Some(ClassItem::Space(true)),
        _ => None,
    }
}

fn literal_escape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        other => other,
    }
}

struct Pattern {
    node: Node,
    groups: usize,
    fold: bool,
}

impl Pattern {
    fn new(source: &str, fold: bool) -> Result<Self, CommandError> {
        let mut parser = Parser { chars: source.chars().collect(), pos: 0, groups: 0 };
        let alts = parser.alts()?;
        if parser.pos != parser.chars.len() {
            return Err(CommandError::invalid_params());
        }
        Ok(Pattern { node: Node::Group(alts, Some(0)), groups: parser.groups + 1, fold })
    }

    fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        self.find_at(&chars, 0).is_some()
    }

    fn replace_all(&self, text: &str, rep: &str, expand: bool) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut pos = 0;
        while pos <= chars.len() {
            let caps = match self.find_at(&chars, pos) {
                Some(caps) => caps,
                None => break,
            };
            let (start, end) = match caps[0] {
                Some(span) => span,
                None => break,
            };
            out.extend(&chars[pos..start]);
            if expand {
                self.expand(rep, &chars, &caps, &mut out);
            } else {
                out.push_str(rep);
            }
            if end == start {
                if start < chars.len() {
                    out.push(chars[start]);
                }
                pos = start + 1;
            } else {
                pos = end;
            }
        }
        if pos < chars.len() {
            out.extend(&chars[pos..]);
        }
        out
    }

    fn expand(&self, rep: &str, chars: &[char], caps: &Caps, out: &mut String) {
        let rep: Vec<char> = rep.chars().collect();
        let mut i = 0;
        while i < rep.len() {
            if rep[i] == '$' && i + 1 < rep.len() {
                let next = rep[i + 1];
                if next == '$' {
                    out.push('$');
                    i += 2;
                    continue;
                }
                if let Some(d) = next.to_digit(10) {
                    if let Some(Some((a, b))) = caps.get(d as usize) {
                        out.extend(&chars[*a..*b]);
                    }
                    i += 2;
                    continue;
                }
            }
            out.push(rep[i]);
            i += 1;
        }
    }

    fn find_at(&self, text: &[char], start: usize) -> Option<Caps> {
        for s in start..=text.len() {
            let mut caps = vec![None; self.groups];
            if self.run(std::slice::from_ref(&self.node), None, text, s, &mut caps) {
                return Some(caps);
            }
        }
        None
    }

    fn run<'a>(&self, nodes: &'a [Node], cont: Option<&'a Cont<'a>>, text: &[char], pos: usize, caps: &mut Caps) -> bool {
        let (node, rest) = match nodes.split_first() {
            Some(split) => split,
            None => return self.resume(cont, text, pos, caps),
        };
        match node {
            Node::Literal(_) | Node::Any | Node::Class(..) => {
                pos < text.len() && self.single(node, text[pos]) && self.run(rest, cont, text, pos + 1, caps)
            }
            Node::Start => pos == 0 && self.run(rest, cont, text, pos, caps),
            Node::End => pos == text.len() && self.run(rest, cont, text, pos, caps),
            Node::Group(alts, cap) => {
                let after = Cont { frame: Frame::Seq(rest), next: cont };
                for alt in alts {
                    let ok = match cap {
                        Some(g) => {
                            let close = Cont { frame: Frame::Close(*g, pos), next: Some(&after) };
                            self.run(alt, Some(&close), text, pos, caps)
                        }
                        None => self.run(alt, Some(&after), text, pos, caps),
                    };
                    if ok {
                        return true;
                    }
                }
                false
            }
            Node::Repeat(inner, min, max, lazy) => {
                let after = Cont { frame: Frame::Seq(rest), next: cont };
                self.again(inner, *min, *max, *lazy, 0, usize::MAX, Some(&after), text, pos, caps)
            }
        }
    }

    fn resume<'a>(&self, cont: Option<&'a Cont<'a>>, text: &[char], pos: usize, caps: &mut Caps) -> bool {
        let c = match cont {
            Some(c) => c,
            None => return true,
        };
        match c.frame {
            Frame::Seq(nodes) => self.run(nodes, c.next, text, pos, caps),
            Frame::Close(g, start) => {
                let old = caps[g];
                caps[g] = Some((start, pos));
                if self.resume(c.next, text, pos, caps) {
                    true
                } else {
                    caps[g] = old;
                    false
                }
            }
            Frame::Again(inner, min, max, lazy, count, last) => {
                self.again(inner, min, max, lazy, count, last, c.next, text, pos, caps)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn again<'a>(
        &self,
        inner: &'a Node,
        min: usize,
        max: Option<usize>,
        lazy: bool,
        count: usize,
        last: usize,
        cont: Option<&'a Cont<'a>>,
        text: &[char],
        pos: usize,
        caps: &mut Caps,
    ) -> bool {
        let more = !matches!(max, Some(m) if count >= m) && (count < min || pos != last);
        let done = count >= min;
        let step = Cont { frame: Frame::Again(inner, min, max, lazy, count + 1, pos), next: cont };
        let one = std::slice::from_ref(inner);
        if !lazy && more && self.run(one, Some(&step), text, pos, caps) {
            return true;
        }
        if done && self.resume(cont, text, pos, caps) {
            return true;
        }
        lazy && more && self.run(one, Some(&step), text, pos, caps)
    }

    fn single(&self, node: &Node, c: char) -> bool {
        match node {
            Node::Literal(l) => char_eq(*l, c, !self.fold),
            Node::Any => c != '\n',
            Node::Class(items, negated) => items.iter().any(|item| self.item(item, c)) != *negated,
            _ => false,
        }
    }

    fn item(&self, item: &ClassItem, c: char) -> bool {
        match item {
            ClassItem::Range(lo, hi) => {
                let inside = |x: char| *lo <= x && x <= *hi;
                inside(c) || (self.fold && (c.to_lowercase().any(inside) || c.to_uppercase().any(inside)))
            }
            ClassItem::Digit(negated) => c.is_ascii_digit() != *negated,
            ClassItem::Word(negated) => (c.is_alphanumeric() || c == '_') != *negated,
            ClassItem::Space(negated) => c.is_whitespace() != *negated,
        }
    }
}
