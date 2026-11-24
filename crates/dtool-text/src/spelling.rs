#[path = "kiwi_ffi.rs"]
pub mod kiwi_ffi;
#[path = "ko_grammar.rs"]
pub mod ko_grammar;
use dtool_core::{Command, CommandError, Session, StoryId};
use spellbook::Dictionary;
use dtool_core::serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const EN_US_AFF: &str = include_str!("../data/hunspell/en_US.aff");
pub const EN_US_DIC: &str = include_str!("../data/hunspell/en_US.dic");
pub const HUNSPELL_DIR_ENV: &str = "DTOOL_HUNSPELL_DIR";
pub const HUNSPELL_BUNDLE_DIR: &str = "hunspell";
pub const KO_DIR: &str = "ko";
pub const KO_AFF_FILE: &str = "ko.aff";
pub const KO_DIC_FILE: &str = "ko.dic";
pub const KO_MISSING_KEY: &str = "spell.ko.dictionary_missing";
pub const KO_INVALID_KEY: &str = "spell.ko.dictionary_invalid";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Hangul,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misspelling {
    pub start: usize,
    pub end: usize,
    pub word: String,
}

pub struct Speller {
    dict: Dictionary,
    ignored: BTreeSet<String>,
    user: BTreeSet<String>,
}

fn dictionary_error() -> CommandError {
    CommandError::new("SPELL_DICTIONARY_UNAVAILABLE")
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}'
}

fn is_hangul(ch: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&ch)
}

pub fn script(word: &str) -> Option<Script> {
    if word.chars().any(|ch| ch.is_ascii_alphabetic()) && word.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '\'') {
        Some(Script::Latin)
    } else if !word.is_empty() && word.chars().all(is_hangul) {
        Some(Script::Hangul)
    } else {
        None
    }
}

pub fn checkable(word: &str) -> bool {
    script(word).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KoDictionaryError {
    Missing(Vec<PathBuf>),
    Invalid(PathBuf, String),
}

impl KoDictionaryError {
    pub fn message_key(&self) -> &'static str {
        match self {
            KoDictionaryError::Missing(_) => KO_MISSING_KEY,
            KoDictionaryError::Invalid(..) => KO_INVALID_KEY,
        }
    }
}

pub struct KoDictionary {
    pub dir: PathBuf,
    pub dict: Dictionary,
}

pub fn hunspell_candidate_dirs() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os(HUNSPELL_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return vec![PathBuf::from(dir)];
    }
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        out.push(dir.join(HUNSPELL_BUNDLE_DIR));
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("data").join(HUNSPELL_BUNDLE_DIR));
    out
}

pub fn ko_dictionary_files(root: &Path) -> (PathBuf, PathBuf) {
    let dir = root.join(KO_DIR);
    (dir.join(KO_AFF_FILE), dir.join(KO_DIC_FILE))
}

pub fn ko_bundle_complete(root: &Path) -> bool {
    let (aff, dic) = ko_dictionary_files(root);
    aff.is_file() && dic.is_file()
}

pub fn hunspell_dir() -> Option<PathBuf> {
    hunspell_candidate_dirs().into_iter().find(|dir| ko_bundle_complete(dir))
}

pub fn load_ko_dictionary(roots: &[PathBuf]) -> Result<KoDictionary, KoDictionaryError> {
    let root = roots.iter().find(|dir| ko_bundle_complete(dir)).ok_or_else(|| KoDictionaryError::Missing(roots.to_vec()))?;
    let (aff_path, dic_path) = ko_dictionary_files(root);
    let invalid = |why: String| KoDictionaryError::Invalid(root.join(KO_DIR), why);
    let aff = std::fs::read_to_string(&aff_path).map_err(|e| invalid(format!("{}: {e}", aff_path.display())))?;
    let dic = std::fs::read_to_string(&dic_path).map_err(|e| invalid(format!("{}: {e}", dic_path.display())))?;
    let count = dic.lines().next().and_then(|line| line.trim().parse::<usize>().ok()).unwrap_or(0);
    if aff.trim().is_empty() || count == 0 {
        return Err(invalid("empty ko.aff or ko.dic".into()));
    }
    let dict = Dictionary::new(&aff, &dic).map_err(|e| invalid(e.to_string()))?;
    Ok(KoDictionary { dir: root.join(KO_DIR), dict })
}

static KO: OnceLock<Result<KoDictionary, KoDictionaryError>> = OnceLock::new();

fn ko_state() -> &'static Result<KoDictionary, KoDictionaryError> {
    KO.get_or_init(|| load_ko_dictionary(&hunspell_candidate_dirs()))
}

pub fn ko_dictionary() -> Option<&'static Dictionary> {
    ko_state().as_ref().ok().map(|ko| &ko.dict)
}

pub fn ko_dictionary_dir() -> Option<&'static Path> {
    ko_state().as_ref().ok().map(|ko| ko.dir.as_path())
}

pub fn ko_dictionary_error() -> Option<KoDictionaryError> {
    ko_state().as_ref().err().cloned()
}

pub fn ko_loaded() -> bool {
    KO.get().is_some()
}

pub fn ko_available() -> bool {
    match KO.get() {
        Some(state) => state.is_ok(),
        None => hunspell_dir().is_some(),
    }
}

pub fn ko_message_key() -> Option<&'static str> {
    match KO.get() {
        Some(Err(error)) => Some(error.message_key()),
        Some(Ok(_)) => None,
        None => hunspell_dir().is_none().then_some(KO_MISSING_KEY),
    }
}

pub fn warm_korean() {
    if !ko_loaded() {
        std::thread::spawn(|| {
            ko_dictionary();
        });
    }
    ko_grammar::warm_kiwi();
}

pub fn without_issue_overlap(marks: Vec<Misspelling>, issues: &[ko_grammar::GrammarIssue]) -> Vec<Misspelling> {
    marks.into_iter().filter(|m| !issues.iter().any(|i| i.overlaps(m.start, m.end))).collect()
}

pub fn story_issues(session: &Session, story: StoryId) -> Vec<ko_grammar::GrammarIssue> {
    let Some(story) = session.document.stories.iter().find(|s| s.id == story) else { return Vec::new() };
    ko_grammar::check(&story.text).into_iter().filter(|i| !doc_ignores(&session.document, &i.text)).collect()
}

pub fn issue_json(story: StoryId, issue: &ko_grammar::GrammarIssue) -> Value {
    json!({ "story": story.0, "start": issue.start, "end": issue.end, "kind": issue.kind.as_str(), "label_key": issue.kind.label_key(), "word": issue.text, "suggestion": issue.suggestion })
}

pub fn words(text: &str) -> Vec<Misspelling> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let mut start = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        let mut end = i;
        while start < end && matches!(chars[start], '\'' | '\u{2019}') {
            start += 1;
        }
        while end > start && matches!(chars[end - 1], '\'' | '\u{2019}') {
            end -= 1;
        }
        if start < end {
            let word: String = chars[start..end].iter().map(|ch| if *ch == '\u{2019}' { '\'' } else { *ch }).collect();
            out.push(Misspelling { start, end, word });
        }
    }
    out
}

impl Speller {
    pub fn from_data(aff: &str, dic: &str) -> Result<Self, CommandError> {
        let count = dic.lines().next().and_then(|line| line.trim().parse::<usize>().ok()).unwrap_or(0);
        if aff.trim().is_empty() || count == 0 {
            return Err(dictionary_error());
        }
        let dict = Dictionary::new(aff, dic).map_err(|_| dictionary_error())?;
        Ok(Self { dict, ignored: BTreeSet::new(), user: BTreeSet::new() })
    }

    pub fn en_us() -> Result<Self, CommandError> {
        let mut speller = Self::from_data(EN_US_AFF, EN_US_DIC)?;
        if let Some(path) = user_dictionary_path() {
            for word in read_user_words(&path) {
                speller.add_user_word(&word);
            }
        }
        Ok(speller)
    }

    pub fn add_user_word(&mut self, word: &str) {
        self.user.insert(word.to_lowercase());
    }

    pub fn has_user_word(&self, word: &str) -> bool {
        self.user.contains(&word.to_lowercase())
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_lowercase());
    }

    pub fn is_correct(&self, word: &str) -> bool {
        let Some(script) = script(word) else { return true };
        let lower = word.to_lowercase();
        if self.ignored.contains(&lower) || self.user.contains(&lower) {
            return true;
        }
        match script {
            Script::Latin => self.dict.check(word),
            Script::Hangul => ko_dictionary().map_or(true, |dict| dict.check(word)),
        }
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        match script(word) {
            Some(Script::Hangul) => {
                if let Some(dict) = ko_dictionary() {
                    dict.suggest(word, &mut out);
                }
            }
            _ => self.dict.suggest(word, &mut out),
        }
        out
    }

    pub fn check_text(&self, text: &str) -> Vec<Misspelling> {
        words(text).into_iter().filter(|item| !self.is_correct(&item.word)).collect()
    }

    pub fn check_story(&self, session: &Session, story: StoryId) -> Result<Vec<Misspelling>, CommandError> {
        let story = session.document.stories.iter().find(|s| s.id == story).ok_or_else(CommandError::not_found)?;
        Ok(self.check_text(&story.text).into_iter().filter(|m| !doc_ignores(&session.document, &m.word)).collect())
    }
}

pub fn replace(session: &mut Session, story: StoryId, item: &Misspelling, with: &str) -> Result<(), CommandError> {
    let text = session.document.stories.iter().find(|s| s.id == story).ok_or_else(CommandError::not_found)?.text.clone();
    let current: String = text.chars().skip(item.start).take(item.end.saturating_sub(item.start)).collect();
    let normalized: String = current.chars().map(|ch| if ch == '\u{2019}' { '\'' } else { ch }).collect();
    if item.end <= item.start || normalized != item.word {
        return Err(CommandError::new("SPELL_STALE"));
    }
    if current == with {
        return Ok(());
    }
    session.execute_batch(vec![dtool_core::text_cmd::range_replace_command(story, item.start, item.end, with)])?;
    Ok(())
}

pub const SPELL_CHECK: &str = "text.spell.check";
pub const SPELL_SUGGEST: &str = "text.spell.suggest";
pub const SPELL_REPLACE: &str = "text.spell.replace";
pub const SPELL_IGNORE: &str = "text.spell.ignore";
pub const SPELL_ADD_WORD: &str = "text.spell.add_word";
pub const SPELL_IGNORE_RESTORE: &str = "text.spell.ignore.restore";
pub const SPELL_COMMANDS: [&str; 6] = [SPELL_CHECK, SPELL_SUGGEST, SPELL_REPLACE, SPELL_IGNORE, SPELL_ADD_WORD, SPELL_IGNORE_RESTORE];

pub fn doc_ignores(doc: &dtool_core::Document, word: &str) -> bool {
    let lower = word.to_lowercase();
    doc.spell_ignore.iter().any(|w| *w == lower)
}
pub const USER_DICTIONARY_FILE: &str = "user_dictionary_en_US.txt";

pub fn user_dictionary_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").filter(|dir| !dir.is_empty()).map(|dir| PathBuf::from(dir).join("DTool").join(USER_DICTIONARY_FILE))
}

pub fn read_user_words(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).map(|text| text.lines().map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

pub fn append_user_word(path: &Path, word: &str) -> Result<bool, CommandError> {
    let word = word.trim();
    if word.is_empty() || word.contains(char::is_whitespace) {
        return Err(CommandError::invalid_params());
    }
    if read_user_words(path).iter().any(|w| w.eq_ignore_ascii_case(word)) {
        return Ok(false);
    }
    let failed = |_| CommandError::new("SPELL_USER_DICTIONARY_WRITE");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(failed)?;
    writeln!(file, "{word}").map_err(failed)?;
    Ok(true)
}

fn shared() -> &'static Mutex<Option<Speller>> {
    static SHARED: OnceLock<Mutex<Option<Speller>>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(Speller::en_us().ok()))
}

pub fn with_shared<R>(f: impl FnOnce(Option<&mut Speller>) -> R) -> R {
    let mut guard = shared().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.as_mut())
}

fn word_param(params: &Value) -> Option<&str> {
    params.get("word").and_then(Value::as_str).filter(|w| !w.trim().is_empty())
}

fn replace_target(params: &Value) -> Option<(StoryId, Misspelling, String)> {
    let get = |key: &str| params.get(key).and_then(Value::as_u64);
    let story = StoryId(get("story")?);
    let item = Misspelling { start: get("start")? as usize, end: get("end")? as usize, word: word_param(params)?.to_string() };
    let with = params.get("with").and_then(Value::as_str)?.to_string();
    Some((story, item, with))
}

fn check(session: &Session, params: &Value) -> Result<Value, CommandError> {
    let ids: Vec<StoryId> = match params.get("story") {
        None | Some(Value::Null) => session.document.stories.iter().map(|s| s.id).collect(),
        Some(value) => vec![StoryId(value.as_u64().ok_or_else(CommandError::invalid_params)?)],
    };
    let grammar = params.get("grammar").and_then(Value::as_bool).unwrap_or(true);
    let mut found = Vec::new();
    for id in &ids {
        let list = if grammar { story_issues(session, *id) } else { Vec::new() };
        found.push((*id, list));
    }
    let issues: Vec<Value> = found.iter().flat_map(|(id, list)| list.iter().map(move |i| issue_json(*id, i))).collect();
    let grammar_available = ko_grammar::kiwi_ready();
    let ko_available = ko_available();
    let ko_message_key = ko_message_key();
    with_shared(|speller| {
        let Some(speller) = speller else {
            return Ok(json!({ "available": false, "marks": [], "issues": issues, "grammar_available": grammar_available, "ko_available": ko_available, "ko_message_key": ko_message_key }));
        };
        let mut marks = Vec::new();
        for (id, list) in &found {
            for item in without_issue_overlap(speller.check_story(session, *id)?, list) {
                marks.push(json!({ "story": id.0, "start": item.start, "end": item.end, "word": item.word }));
            }
        }
        Ok(json!({ "available": true, "marks": marks, "issues": issues, "grammar_available": grammar_available, "ko_available": ko_available, "ko_message_key": ko_message_key }))
    })
}

fn ignore_in_document(session: &mut Session, word: &str) -> Result<Value, CommandError> {
    let lower = word.trim().to_lowercase();
    if session.document.spell_ignore.contains(&lower) {
        return Ok(json!({ "ignored": lower, "scope": "document", "changed": false, "doc_version": session.generation }));
    }
    let before = session.document.spell_ignore.clone();
    session.document.spell_ignore.push(lower.clone());
    let after = session.document.spell_ignore.clone();
    let restore = |list: &[String]| Command::with_params(SPELL_IGNORE_RESTORE, json!({ "list": list }));
    dtool_core::text_cmd::push_change(session, SPELL_IGNORE, restore(&after), restore(&before));
    Ok(json!({ "ignored": lower, "scope": "document", "changed": true, "doc_version": session.generation }))
}

fn restore_ignores(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let list: Vec<String> = params.get("list").and_then(Value::as_array).ok_or_else(CommandError::invalid_params)?.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(CommandError::invalid_params)).collect::<Result<_, _>>()?;
    session.document.spell_ignore = list;
    session.document.version = session.document.version.saturating_add(1);
    Ok(json!({}))
}

fn add_word(word: &str) -> Result<Value, CommandError> {
    let path = user_dictionary_path().ok_or_else(|| CommandError::new("SPELL_USER_DICTIONARY_WRITE"))?;
    let added = append_user_word(&path, word)?;
    with_shared(|speller| {
        if let Some(speller) = speller {
            speller.add_user_word(word.trim());
        }
    });
    Ok(json!({ "word": word.trim(), "added": added, "path": path.to_string_lossy() }))
}

pub fn run_spell_command(session: &mut Session, command: &Command) -> Option<Result<Value, CommandError>> {
    let name = command.name.as_str();
    if !SPELL_COMMANDS.contains(&name) {
        return None;
    }
    let params = &command.params;
    Some(match name {
        SPELL_CHECK if !session.has_document => Err(CommandError::no_document()),
        SPELL_CHECK => check(session, params),
        SPELL_SUGGEST => match word_param(params) {
            None => Err(CommandError::invalid_params()),
            Some(word) => with_shared(|speller| {
                let speller = speller.ok_or_else(dictionary_error)?;
                Ok(json!({ "word": word, "correct": speller.is_correct(word), "suggestions": speller.suggest(word) }))
            }),
        },
        SPELL_IGNORE_RESTORE => restore_ignores(session, params),
        SPELL_IGNORE if session.has_document && params.get("scope").and_then(Value::as_str).unwrap_or("document") == "document" => match word_param(params) {
            None => Err(CommandError::invalid_params()),
            Some(word) => ignore_in_document(session, word),
        },
        SPELL_IGNORE => match word_param(params) {
            None => Err(CommandError::invalid_params()),
            Some(word) => with_shared(|speller| {
                let speller = speller.ok_or_else(dictionary_error)?;
                speller.ignore(word);
                Ok(json!({ "ignored": word.to_lowercase() }))
            }),
        },
        SPELL_ADD_WORD => match word_param(params) {
            None => Err(CommandError::invalid_params()),
            Some(word) => add_word(word),
        },
        _ if !session.has_document => Err(CommandError::no_document()),
        _ => match replace_target(params) {
            None => Err(CommandError::invalid_params()),
            Some((story, item, with)) => replace(session, story, &item, &with).map(|_| json!({ "doc_version": session.generation })),
        },
    })
}

pub fn can_spell(session: &Session, command: &Command) -> Option<bool> {
    let params = &command.params;
    Some(match command.name.as_str() {
        SPELL_CHECK => session.has_document,
        SPELL_SUGGEST | SPELL_IGNORE | SPELL_ADD_WORD => word_param(params).is_some(),
        SPELL_IGNORE_RESTORE => false,
        SPELL_REPLACE => session.has_document && replace_target(params).is_some(),
        _ => return None,
    })
}
