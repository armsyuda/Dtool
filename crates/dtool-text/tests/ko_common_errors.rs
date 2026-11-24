use dtool_text::spelling::ko_grammar::{self, IssueKind, MatchMode};

#[test]
fn every_listed_pair_is_flagged_with_its_correction() {
    let cases = [
        ("외않되?", "외않되", "왜 안 돼"),
        ("그러면 않되.", "않되", "안 돼"),
        ("그건 않되요", "않되요", "안 돼요"),
        ("이건 않돼", "않돼", "안 돼"),
        ("몇일 동안", "몇일", "며칠"),
        ("금새 사라졌다", "금새", "금세"),
        ("설겆이를 했다", "설겆이", "설거지"),
        ("웬지 좋다", "웬지", "왠지"),
        ("정말 어의없다", "어의없", "어이없"),
        ("역활을 맡았다", "역활", "역할"),
        ("일일히 확인했다", "일일히", "일일이"),
        ("희안하다", "희안", "희한"),
        ("오랫만이야", "오랫만", "오랜만"),
        ("다 됬다", "됬", "됐"),
        ("내가 할께", "할께", "할게"),
        ("이거 줄께요", "줄께", "줄게"),
        ("먼저 갈께", "갈께", "갈게"),
        ("그러면 안되요", "되요", "돼요"),
        ("어떻해", "어떻해", "어떡해"),
        ("왠만하면 와", "왠만", "웬만"),
    ];
    for (text, wrong, right) in cases {
        let issues = ko_grammar::check_common(text);
        assert!(issues.iter().any(|i| i.kind == IssueKind::Grammar && i.text == wrong && i.suggestion == right), "{text}: {issues:?}");
        let chars: Vec<char> = text.chars().collect();
        for issue in &issues {
            assert_eq!(chars[issue.start..issue.end].iter().collect::<String>(), issue.text, "{text}");
        }
    }
}

#[test]
fn josa_and_endings_after_a_listed_stem_still_match() {
    for (text, wrong) in [("몇일이 지났다", "몇일"), ("설겆이는 내가", "설겆이"), ("역활이 크다", "역활"), ("어의없는 일", "어의없"), ("할께요", "할께"), ("안됬어", "됬"), ("희안한 일", "희안")] {
        let issues = ko_grammar::check_common(text);
        assert_eq!(issues.len(), 1, "{text}: {issues:?}");
        assert_eq!(issues[0].text, wrong, "{text}");
    }
}

#[test]
fn connective_anhdoe_mid_sentence_is_not_flagged() {
    assert!(ko_grammar::check_common("가지 않되 연락은 해라").is_empty());
}

#[test]
fn correct_text_has_no_list_hits() {
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
        "왠지 기분이 좋다.",
        "금세 끝났다.",
        "며칠 동안 기다렸다.",
        "설거지를 했다.",
        "선생님께 드렸다.",
        "오랜만에 만났다.",
        "역할을 나눴다.",
        "일이 잘 됐다.",
    ];
    let hits: Vec<_> = correct.iter().map(|t| (t, ko_grammar::check_common(t))).filter(|(_, v)| !v.is_empty()).collect();
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn default_list_is_embedded_and_user_file_extends_and_overrides() {
    let base = ko_grammar::parse_common_errors(ko_grammar::COMMON_ERRORS_DEFAULT);
    assert!(base.len() >= 20);
    assert!(base.iter().any(|e| e.wrong == "됬" && e.mode == MatchMode::Any));
    let dir = std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA")).join("DTool");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ko_common_errors_test_override.tsv");
    std::fs::write(&path, "몇일\t며칠날\n짜집기\t짜깁기\tany\n").unwrap();
    let list = ko_grammar::load_common_errors(Some(&path));
    assert_eq!(list.len(), base.len() + 1);
    assert!(list.iter().any(|e| e.wrong == "짜집기" && e.right == "짜깁기" && e.mode == MatchMode::Any));
    assert!(list.iter().any(|e| e.wrong == "몇일" && e.right == "며칠날"));
    assert!(list.windows(2).all(|w| w[0].wrong.chars().count() >= w[1].wrong.chars().count()));
    assert_eq!(ko_grammar::load_common_errors(None).len(), base.len());
}
