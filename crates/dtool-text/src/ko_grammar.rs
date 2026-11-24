use super::kiwi_ffi::{Kiwi, LoadError, Token, LIB_FILE, MODEL_DIR};
use super::Misspelling;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const KIWI_DIR_ENV: &str = "DTOOL_KIWI_DIR";
pub const BUNDLE_DIR: &str = "kiwi";
pub const TYPO_COST_WEIGHT: f32 = 5.0;
pub const TYPO_THRESHOLD: f32 = 2.5;
pub const THREADS: i32 = 1;
pub const DEFAULT_IDLE_UNLOAD: Duration = Duration::from_secs(300);
pub const COMMON_ERRORS_DEFAULT: &str = include_str!("../data/ko_common_errors.tsv");
pub const COMMON_ERRORS_FILE: &str = "ko_common_errors.tsv";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueKind {
    Spacing,
    Grammar,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::Spacing => "spacing",
            IssueKind::Grammar => "grammar",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            IssueKind::Spacing => "spell.issue.spacing",
            IssueKind::Grammar => "spell.issue.grammar",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarIssue {
    pub start: usize,
    pub end: usize,
    pub kind: IssueKind,
    pub text: String,
    pub suggestion: String,
}

impl GrammarIssue {
    pub fn as_mark(&self) -> Misspelling {
        Misspelling { start: self.start, end: self.end, word: self.text.clone() }
    }

    pub fn contains(&self, index: usize) -> bool {
        self.start <= index && index < self.end
    }

    pub fn overlaps(&self, start: usize, end: usize) -> bool {
        self.start < end && start < self.end
    }

    pub fn covers(&self, start: usize, end: usize) -> bool {
        self.start <= start && end <= self.end
    }
}

pub struct KoGrammar {
    pub kiwi: Kiwi,
    pub dir: PathBuf,
    pub load_time: Duration,
}

type Job = Box<dyn FnOnce(&KoGrammar) + Send>;

#[derive(Clone)]
pub struct KiwiWorker {
    jobs: mpsc::Sender<Job>,
}

impl KiwiWorker {
    pub fn run<R: Send + 'static>(&self, f: impl FnOnce(&KoGrammar) -> R + Send + 'static) -> Option<R> {
        let (reply, answer) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |grammar| {
            let _ = reply.send(f(grammar));
        });
        self.jobs.send(job).ok()?;
        answer.recv().ok()
    }
}

static RESIDENT: AtomicUsize = AtomicUsize::new(0);

struct Resident;

impl Resident {
    fn enter() -> Resident {
        RESIDENT.fetch_add(1, Ordering::SeqCst);
        Resident
    }
}

impl Drop for Resident {
    fn drop(&mut self) {
        RESIDENT.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn_worker() -> Result<(KiwiWorker, Duration), LoadError> {
    let (jobs, queue) = mpsc::channel::<Job>();
    let (ready, started) = mpsc::sync_channel::<Result<Duration, LoadError>>(1);
    let resident = Resident::enter();
    std::thread::Builder::new()
        .name("dtool-kiwi".into())
        .spawn(move || {
            let _resident = resident;
            let grammar = match load() {
                Ok(grammar) => grammar,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let _ = ready.send(Ok(grammar.load_time));
            for job in queue {
                job(&grammar);
            }
            drop(grammar);
        })
        .map_err(|e| LoadError::Init(e.to_string()))?;
    let load_time = started.recv().map_err(|e| LoadError::Init(e.to_string()))??;
    Ok((KiwiWorker { jobs }, load_time))
}

struct State {
    engine: Option<KiwiWorker>,
    error: Option<LoadError>,
    loading: bool,
    warming: bool,
    reaper: bool,
    last_used: Option<Instant>,
    idle: Option<Duration>,
    load_time: Option<Duration>,
    loads: u32,
    unloads: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    engine: None,
    error: None,
    loading: false,
    warming: false,
    reaper: false,
    last_used: None,
    idle: Some(DEFAULT_IDLE_UNLOAD),
    load_time: None,
    loads: 0,
    unloads: 0,
});
static READY: Condvar = Condvar::new();

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = std::env::var_os(KIWI_DIR_ENV).filter(|dir| !dir.is_empty()) {
        out.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        out.push(dir.join(BUNDLE_DIR));
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("data").join(BUNDLE_DIR));
    out
}

pub fn bundle_complete(dir: &Path) -> bool {
    dir.join(LIB_FILE).is_file() && dir.join(MODEL_DIR).is_dir()
}

pub fn kiwi_dir() -> Option<PathBuf> {
    candidate_dirs().into_iter().find(|dir| bundle_complete(dir))
}

fn load() -> Result<KoGrammar, LoadError> {
    let Some(dir) = kiwi_dir() else {
        let first = candidate_dirs().into_iter().next().unwrap_or_default();
        return Err(LoadError::LibraryMissing(first.join(LIB_FILE)));
    };
    let started = Instant::now();
    let kiwi = Kiwi::load(&dir, THREADS, TYPO_COST_WEIGHT)?;
    Ok(KoGrammar { kiwi, dir, load_time: started.elapsed() })
}

pub fn engine() -> Option<KiwiWorker> {
    let mut s = state();
    loop {
        if let Some(engine) = s.engine.clone() {
            s.last_used = Some(Instant::now());
            return Some(engine);
        }
        if s.error.is_some() {
            return None;
        }
        if !s.loading {
            break;
        }
        s = READY.wait(s).unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    s.loading = true;
    drop(s);
    let result = spawn_worker();
    let mut s = state();
    s.loading = false;
    s.warming = false;
    match result {
        Ok((worker, load_time)) => {
            s.load_time = Some(load_time);
            s.engine = Some(worker);
            s.loads += 1;
            s.last_used = Some(Instant::now());
            start_reaper(&mut s);
        }
        Err(error) => s.error = Some(error),
    }
    READY.notify_all();
    s.engine.clone()
}

fn reaper_wait(idle: Option<Duration>) -> Duration {
    idle.map_or(Duration::from_secs(5), |limit| (limit / 10).clamp(Duration::from_millis(10), Duration::from_secs(5)))
}

fn start_reaper(s: &mut State) {
    if s.reaper {
        return;
    }
    s.reaper = true;
    std::thread::spawn(|| loop {
        let wait = reaper_wait(state().idle);
        std::thread::sleep(wait);
        let mut s = state();
        let released = unload_idle_locked(&mut s, Instant::now());
        let done = s.engine.is_none();
        if done {
            s.reaper = false;
        }
        drop(s);
        drop(released);
        if done {
            return;
        }
    });
}

fn unload_idle_locked(s: &mut State, now: Instant) -> Option<KiwiWorker> {
    let (Some(limit), Some(last)) = (s.idle, s.last_used) else { return None };
    if s.engine.is_none() || now.saturating_duration_since(last) < limit {
        return None;
    }
    s.unloads += 1;
    s.engine.take()
}

pub fn unload_if_idle(now: Instant) -> bool {
    let released = unload_idle_locked(&mut state(), now);
    released.is_some()
}

pub fn unload() -> bool {
    let released = {
        let mut s = state();
        let released = s.engine.take();
        if released.is_some() {
            s.unloads += 1;
        }
        released
    };
    released.is_some()
}

pub fn set_idle_unload(limit: Option<Duration>) {
    state().idle = limit;
}

pub fn set_idle_unload_secs(secs: u32) {
    set_idle_unload((secs > 0).then(|| Duration::from_secs(u64::from(secs))));
}

pub fn idle_unload() -> Option<Duration> {
    state().idle
}

pub fn load_count() -> u32 {
    state().loads
}

pub fn unload_count() -> u32 {
    state().unloads
}

pub fn kiwi_loaded() -> bool {
    let s = state();
    s.engine.is_some() || s.error.is_some()
}

pub fn kiwi_ready() -> bool {
    state().engine.is_some()
}

pub fn kiwi_resident() -> bool {
    RESIDENT.load(Ordering::SeqCst) > 0
}

pub fn kiwi_loading() -> bool {
    state().loading
}

pub fn load_error() -> Option<LoadError> {
    state().error.clone()
}

pub fn load_time() -> Option<Duration> {
    state().load_time
}

pub fn version() -> Option<String> {
    with_engine(|engine| engine.kiwi.version())
}

pub fn warm_kiwi() {
    {
        let mut s = state();
        if s.engine.is_some() || s.error.is_some() || s.loading || s.warming {
            return;
        }
        s.warming = true;
    }
    std::thread::spawn(|| {
        engine();
    });
}

pub fn with_engine<R: Send + 'static>(f: impl FnOnce(&KoGrammar) -> R + Send + 'static) -> Option<R> {
    engine()?.run(f)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Prefix,
    Any,
    Exact,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonError {
    pub wrong: String,
    pub right: String,
    pub mode: MatchMode,
}

pub fn parse_common_errors(text: &str) -> Vec<CommonError> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') {
                return None;
            }
            let mut cols = line.split('\t');
            let wrong = cols.next()?.trim();
            let right = cols.next()?.trim();
            let mode = match cols.next().map(str::trim).unwrap_or("") {
                "any" => MatchMode::Any,
                "exact" => MatchMode::Exact,
                "final" => MatchMode::Final,
                _ => MatchMode::Prefix,
            };
            (!wrong.is_empty() && !right.is_empty() && wrong != right).then(|| CommonError { wrong: wrong.to_string(), right: right.to_string(), mode })
        })
        .collect()
}

pub fn merge_common_errors(base: Vec<CommonError>, extra: Vec<CommonError>) -> Vec<CommonError> {
    let mut out: Vec<CommonError> = base.into_iter().filter(|b| !extra.iter().any(|e| e.wrong == b.wrong)).collect();
    out.extend(extra);
    out.sort_by(|a, b| b.wrong.chars().count().cmp(&a.wrong.chars().count()).then_with(|| a.wrong.cmp(&b.wrong)));
    out
}

pub fn common_errors_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").filter(|dir| !dir.is_empty()).map(|dir| PathBuf::from(dir).join("DTool").join(COMMON_ERRORS_FILE))
}

pub fn load_common_errors(user_file: Option<&Path>) -> Vec<CommonError> {
    let extra = user_file.and_then(|path| std::fs::read_to_string(path).ok()).map(|text| parse_common_errors(&text)).unwrap_or_default();
    merge_common_errors(parse_common_errors(COMMON_ERRORS_DEFAULT), extra)
}

static COMMON: Mutex<Option<Arc<Vec<CommonError>>>> = Mutex::new(None);

pub fn common_errors() -> Arc<Vec<CommonError>> {
    let mut guard = COMMON.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(|| Arc::new(load_common_errors(common_errors_path().as_deref()))).clone()
}

pub fn reload_common_errors() {
    *COMMON.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

fn common_issues(list: &[CommonError], chars: &[char], base: usize) -> Vec<GrammarIssue> {
    let runs = word_runs(chars);
    let mut out = Vec::new();
    for (index, &(start, end)) in runs.iter().enumerate() {
        let mut cs = start;
        while cs < end && !chars[cs].is_alphanumeric() {
            cs += 1;
        }
        let mut ce = end;
        while ce > cs && !chars[ce - 1].is_alphanumeric() {
            ce -= 1;
        }
        if cs >= ce {
            continue;
        }
        let core = &chars[cs..ce];
        let is_final = ce < end || index + 1 == runs.len();
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for entry in list {
            let wrong: Vec<char> = entry.wrong.chars().collect();
            if wrong.len() > core.len() {
                continue;
            }
            let hits: Vec<usize> = match entry.mode {
                MatchMode::Prefix => core.starts_with(&wrong).then_some(0).into_iter().collect(),
                MatchMode::Exact => (core == wrong.as_slice()).then_some(0).into_iter().collect(),
                MatchMode::Final => (is_final && core == wrong.as_slice()).then_some(0).into_iter().collect(),
                MatchMode::Any => (0..=core.len() - wrong.len()).filter(|i| core[*i..*i + wrong.len()] == wrong[..]).collect(),
            };
            for hit in hits {
                let (a, b) = (cs + hit, cs + hit + wrong.len());
                if taken.iter().any(|(x, y)| a < *y && *x < b) {
                    continue;
                }
                taken.push((a, b));
                out.push(GrammarIssue { start: base + a, end: base + b, kind: IssueKind::Grammar, text: entry.wrong.clone(), suggestion: entry.right.clone() });
            }
        }
    }
    out.sort_by_key(|issue| issue.start);
    out
}

pub fn check_common(text: &str) -> Vec<GrammarIssue> {
    let list = common_errors();
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for_each_segment(&chars, |segment, base| out.extend(common_issues(&list, segment, base)));
    out
}

pub fn is_hangul(ch: char) -> bool {
    matches!(ch, '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

pub fn has_hangul(text: &str) -> bool {
    text.chars().any(is_hangul)
}

pub fn check(text: &str) -> Vec<GrammarIssue> {
    if !has_hangul(text) {
        return Vec::new();
    }
    let owned = text.to_string();
    engine().and_then(|worker| worker.run(move |grammar| check_with(Some(&grammar.kiwi), &owned))).unwrap_or_else(|| check_with(None, text))
}

pub fn check_if_ready(text: &str) -> Option<Vec<GrammarIssue>> {
    if !has_hangul(text) {
        return Some(Vec::new());
    }
    if !kiwi_loaded() {
        warm_kiwi();
        return None;
    }
    Some(check(text))
}

fn is_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}' | '\u{000B}' | '\u{000C}' | '\u{0003}')
}

fn for_each_segment(chars: &[char], mut f: impl FnMut(&[char], usize)) {
    let mut from = 0;
    for i in 0..=chars.len() {
        if i == chars.len() || is_break(chars[i]) {
            if i > from && chars[from..i].iter().any(|ch| is_hangul(*ch)) {
                f(&chars[from..i], from);
            }
            from = i + 1;
        }
    }
}

pub fn check_with(kiwi: Option<&Kiwi>, text: &str) -> Vec<GrammarIssue> {
    let list = common_errors();
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for_each_segment(&chars, |segment, base| check_segment(kiwi, &list, segment, base, &mut out));
    out
}

fn check_segment(kiwi: Option<&Kiwi>, list: &[CommonError], chars: &[char], base: usize, out: &mut Vec<GrammarIssue>) {
    let common = common_issues(list, chars, base);
    let mut found = common.clone();
    if let Some(kiwi) = kiwi {
        let text: String = chars.iter().collect();
        let spacing = spacing_issues(kiwi, chars, &text, base);
        let grammar = grammar_issues(kiwi, chars, &text, base);
        let clear = |issue: &GrammarIssue| !common.iter().any(|c| c.overlaps(issue.start, issue.end));
        found.extend(grammar.iter().filter(|g| clear(g)).cloned());
        found.extend(spacing.into_iter().filter(|s| clear(s) && !grammar.iter().any(|g| g.overlaps(s.start, s.end))));
    }
    found.sort_by_key(|issue| issue.start);
    out.extend(found);
}

fn known_word(word: &str) -> bool {
    let core = word.trim_matches(|ch: char| !ch.is_alphanumeric());
    !core.is_empty() && super::ko_dictionary().is_some_and(|dict| dict.check(core))
}

fn word_runs(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        out.push((start, i));
    }
    out
}

fn spacing_issues(kiwi: &Kiwi, chars: &[char], text: &str, base: usize) -> Vec<GrammarIssue> {
    let Some(spaced) = kiwi.space(text, false) else { return Vec::new() };
    let target: Vec<char> = spaced.chars().collect();
    let mut inserts = vec![false; chars.len()];
    let mut j = 0;
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_whitespace() {
            while j < target.len() && target[j].is_whitespace() {
                j += 1;
            }
            continue;
        }
        let mut gap = false;
        while j < target.len() && target[j].is_whitespace() {
            gap = true;
            j += 1;
        }
        if j >= target.len() || target[j] != ch {
            return Vec::new();
        }
        if gap && i > 0 && is_hangul(chars[i - 1]) && is_hangul(ch) {
            inserts[i] = true;
        }
        j += 1;
    }
    let mut out = Vec::new();
    for (start, end) in word_runs(chars) {
        if !(start + 1..end).any(|k| inserts[k]) {
            continue;
        }
        let word: String = chars[start..end].iter().collect();
        if known_word(&word) {
            continue;
        }
        let positions: Vec<usize> = (start + 1..end).filter(|k| inserts[*k]).collect();
        let Some(groups) = standalone_pieces(kiwi, &spaced_word(chars, start, end, &positions)) else { continue };
        let kept: Vec<usize> = positions.iter().enumerate().filter(|(gap, _)| !(is_noun(last_tag(&groups[*gap])) && is_noun(first_tag(&groups[*gap + 1])))).map(|(_, k)| *k).collect();
        if kept.is_empty() {
            continue;
        }
        let suggestion = spaced_word(chars, start, end, &kept);
        out.push(GrammarIssue { start: base + start, end: base + end, kind: IssueKind::Spacing, text: word, suggestion });
    }
    out
}

fn is_bound_start(tag: &str) -> bool {
    tag.starts_with('E') || tag.starts_with('J') || tag.starts_with("XS")
}

fn is_bound_end(tag: &str) -> bool {
    tag.starts_with('V') || tag.starts_with("XSV") || tag.starts_with("XSA") || tag == "XPN"
}

fn is_noun(tag: &str) -> bool {
    matches!(tag, "NNG" | "NNP" | "XR")
}

fn first_tag(group: &[Token]) -> &str {
    group.first().map_or("", |t| t.tag.as_str())
}

fn last_tag(group: &[Token]) -> &str {
    group.last().map_or("", |t| t.tag.as_str())
}

fn spaced_word(chars: &[char], start: usize, end: usize, positions: &[usize]) -> String {
    let mut out = String::new();
    for k in start..end {
        if positions.contains(&k) {
            out.push(' ');
        }
        out.push(chars[k]);
    }
    out
}

fn standalone_pieces(kiwi: &Kiwi, suggestion: &str) -> Option<Vec<Vec<Token>>> {
    let tokens = kiwi.analyze(suggestion, false, 0.0);
    let groups = group_by_word(&tokens);
    let valid = groups.len() == suggestion.split_whitespace().count() && groups.iter().all(|group| !is_bound_start(first_tag(group)) && !is_bound_end(last_tag(group)));
    valid.then_some(groups)
}

fn group_by_word(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut out: Vec<Vec<Token>> = Vec::new();
    for token in tokens {
        match out.last_mut() {
            Some(group) if group[0].word == token.word => group.push(token.clone()),
            _ => out.push(vec![token.clone()]),
        }
    }
    out
}

fn utf16_to_char(chars: &[char]) -> Vec<usize> {
    let mut map = Vec::with_capacity(chars.len() + 1);
    for (index, ch) in chars.iter().enumerate() {
        for _ in 0..ch.len_utf16() {
            map.push(index);
        }
    }
    map.push(chars.len());
    map
}

fn grammar_issues(kiwi: &Kiwi, chars: &[char], text: &str, base: usize) -> Vec<GrammarIssue> {
    let tokens = kiwi.analyze(text, true, TYPO_THRESHOLD);
    if !tokens.iter().any(|t| t.typo_cost > 0.0) {
        return Vec::new();
    }
    let map = utf16_to_char(chars);
    let at = |offset: usize| map[offset.min(map.len() - 1)];
    let mut out = Vec::new();
    for group in group_by_word(&tokens) {
        if !group.iter().any(|t| t.typo_cost > 0.0) {
            continue;
        }
        let token_start = at(group.iter().map(|t| t.position).min().unwrap_or(0));
        let token_end = at(group.iter().map(|t| t.position + t.length).max().unwrap_or(0));
        if token_start >= token_end {
            continue;
        }
        let mut start = token_start;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let mut end = token_end;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        let morphs: Vec<(&str, &str)> = group.iter().map(|t| (t.form.as_str(), t.tag.as_str())).collect();
        let Some(joined) = kiwi.join(&morphs).map(|j| j.split_whitespace().collect::<String>()) else { continue };
        let original: String = chars[start..end].iter().collect();
        let suggestion: String = chars[start..token_start].iter().collect::<String>() + &joined + &chars[token_end..end].iter().collect::<String>();
        let squash = |s: &str| s.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
        if joined.is_empty() || suggestion == original || squash(&suggestion) == squash(&original) {
            continue;
        }
        out.push(GrammarIssue { start: base + start, end: base + end, kind: IssueKind::Grammar, text: original, suggestion });
    }
    out
}
