use dtool_text::spelling::ko_grammar::{self, IssueKind};

fn kiwi_or_skip() -> bool {
    if ko_grammar::kiwi_dir().is_some() {
        return true;
    }
    assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing in {:?}", ko_grammar::candidate_dirs());
    eprintln!("KIWI_SKIP bundle not found in {:?}", ko_grammar::candidate_dirs());
    false
}

#[test]
fn run_together_sentence_gets_one_spacing_suggestion() {
    if !kiwi_or_skip() {
        return;
    }
    let issues = ko_grammar::check("아버지가방에들어가신다");
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].kind, IssueKind::Spacing);
    assert_eq!((issues[0].start, issues[0].end), (0, 11));
    assert_eq!(issues[0].text, "아버지가방에들어가신다");
    assert_eq!(issues[0].suggestion, "아버지가 방에 들어가신다");
}

#[test]
fn correct_korean_is_not_flagged() {
    if !kiwi_or_skip() {
        return;
    }
    let correct = [
        "아버지가 방에 들어가신다.",
        "오늘 날씨가 정말 좋네요.",
        "나는 학교에 갔다.",
        "디자인 툴에서 텍스트를 편집합니다.",
        "로봇이 문을 열었다.",
        "우리는 내일 만나기로 했다.",
        "그렇게 하면 안 돼요.",
        "할 수 있다.",
        "이 문서는 세 개의 단락으로 구성되어 있습니다.",
        "Hello 세계",
        "3개를 샀다.",
        "회의는 오후 2시에 시작합니다.",
        "사용자가 글꼴을 바꾸면 미리 보기가 갱신된다.",
        "봄이 오면 꽃이 핀다.",
        "레이어 패널에서 그룹을 선택하세요.",
        "텍스트프레임을 선택하세요.",
        "레이어패널을 엽니다.",
        "어떻게든지 해 볼게요.",
    ];
    let flagged: Vec<_> = correct.iter().map(|t| (t, ko_grammar::check(t))).filter(|(_, v)| !v.is_empty()).collect();
    assert!(flagged.is_empty(), "{flagged:?}");
}

#[test]
fn offsets_are_story_char_indices_across_paragraphs() {
    if !kiwi_or_skip() {
        return;
    }
    let text = "첫 줄입니다.\n우리는내일만나기로했다";
    let issues = ko_grammar::check(text);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!((issues[0].start, issues[0].end), (8, 19));
    let chars: Vec<char> = text.chars().collect();
    assert_eq!(chars[issues[0].start..issues[0].end].iter().collect::<String>(), issues[0].text);
    assert_eq!(issues[0].suggestion, "우리는 내일 만나기로 했다");
}

#[test]
fn typo_hints_come_back_as_grammar_issues() {
    if !kiwi_or_skip() {
        return;
    }
    let issues = ko_grammar::check("잘했따");
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].kind, IssueKind::Grammar, "{issues:?}");
    assert_eq!((issues[0].start, issues[0].end), (0, 3));
    assert_eq!(issues[0].suggestion, "잘했다");
    let issues = ko_grammar::check("나 죰 도와죠.");
    assert!(issues.iter().all(|i| i.kind == IssueKind::Grammar), "{issues:?}");
    assert!(issues.iter().any(|i| i.text == "죰" && i.suggestion == "좀"), "{issues:?}");
    assert!(issues.iter().any(|i| i.text == "도와죠." && i.suggestion == "도와줘."), "{issues:?}");
}

#[test]
fn common_list_wins_over_kiwi_spacing_for_overlapping_ranges() {
    if !kiwi_or_skip() {
        return;
    }
    let issues = ko_grammar::check("몇일 동안 기다렸어요");
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].kind, IssueKind::Grammar);
    assert_eq!((issues[0].start, issues[0].end), (0, 2));
    assert_eq!(issues[0].suggestion, "며칠");
    let issues = ko_grammar::check("외않되?");
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].suggestion, "왜 안 돼");
}

#[test]
fn bound_stem_fragments_are_not_suggested() {
    if !kiwi_or_skip() {
        return;
    }
    let issues = ko_grammar::check("어떻해 해야 해");
    assert!(issues.iter().all(|i| i.suggestion != "어떻 해"), "{issues:?}");
}

#[test]
fn english_only_text_yields_no_issues() {
    assert!(ko_grammar::check("hello wrold, nothing Korean here").is_empty());
    assert!(!ko_grammar::has_hangul("hello"));
}
