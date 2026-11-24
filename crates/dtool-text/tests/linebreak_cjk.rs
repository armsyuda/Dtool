use dtool_text::{CharStyle, FontLibrary, Line, Run, Story, TextRange, layout_story};

struct Fixture {
    library: FontLibrary,
    style: CharStyle,
}
impl Fixture {
    fn new() -> Self {
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dtool-ui/assets/fonts/pretendard/Pretendard-Regular.ttf");
        assert!(path.is_file(),"missing required fixture font: {}",path.display());
        let library=FontLibrary::from_fixtures(&[path]).expect("fixture font loading");
        let family=library.families().into_iter().next().expect("fixture family").to_owned();
        let style=library.faces(&family).into_iter().next().expect("fixture face").style.clone();
        Self {library,style:CharStyle {family,style,size_pt:12.0,..CharStyle::default()}}
    }
    fn layout(&self,text:&str,width:Option<f32>) -> Vec<Line> {
        let length=text.chars().count();
        let story=Story {text:text.into(),chars:vec![Run {range:TextRange::new(0,length),style:self.style.clone()}],..Story::default()};
        let lines=layout_story(&self.library,&story,width);
        assert!(!lines.is_empty(),"nonempty text must produce lines");
        for line in &lines {
            assert!(line.range.start<line.range.end && line.range.end<=length,"invalid scalar range: {:?}",line.range);
            assert!(line.width.is_finite() && line.baseline.is_finite());
        }
        lines
    }
    fn width(&self,text:&str) -> f32 {
        let lines=self.layout(text,None);assert_eq!(lines.len(),1);
        assert!(lines[0].width>0.0,"fixture needs measurable glyphs");lines[0].width
    }
}
fn visible_text(text:&str,line:&Line) -> String {
    text.chars().skip(line.range.start).take(line.range.end-line.range.start).collect::<String>().trim().to_owned()
}
#[test]
fn hangul_breaks_between_syllables() {
    let fixture=Fixture::new();let text="\u{ac00}\u{b098}\u{b2e4}\u{b77c}\u{b9c8}\u{bc14}";
    let width=fixture.width("\u{ac00}")*2.25;let lines=fixture.layout(text,Some(width));
    assert!(lines.len()>1,"Hangul syllables must offer break opportunities");
    let mut covered=0;
    for line in &lines {assert_eq!(line.range.start,covered);covered=line.range.end;}
    assert_eq!(covered,text.chars().count(),"all syllables must remain in order");
    for line in lines.iter().take(lines.len()-1) {assert!(line.width<=width+0.01,"syllable break must fit the frame");}
}
#[test]
fn closing_punctuation_never_starts_a_line() {
    let fixture=Fixture::new();let unit=fixture.width("\u{ac00}");
    for closing in [')','\u{300d}','.',',','!','?','\u{30fc}'] {
        let text=format!("\u{ac00}\u{b098}\u{b2e4}\u{b77c}{closing}\u{b9c8}\u{bc14}");
        for scale in [1.25,2.25,3.25] {
            let lines=fixture.layout(&text,Some(unit*scale));assert!(lines.len()>1);
            assert!(lines.iter().any(|line|visible_text(&text,line).contains(closing)),"closing mark must not disappear");
            for line in &lines {assert_ne!(visible_text(&text,line).chars().next(),Some(closing),"closing mark at line start: U+{:04X}",closing as u32);}
        }
    }
}
#[test]
fn opening_bracket_never_ends_a_line() {
    let fixture=Fixture::new();let unit=fixture.width("\u{ac00}");
    for opening in ['(','\u{300c}'] {
        let text=format!("\u{ac00}\u{b098}{opening}\u{b2e4}\u{b77c}\u{b9c8}\u{bc14}");
        for scale in [1.25,2.25,3.25] {
            let lines=fixture.layout(&text,Some(unit*scale));assert!(lines.len()>1);
            assert!(lines.iter().any(|line|visible_text(&text,line).contains(opening)),"opening bracket must not disappear");
            for line in &lines {assert_ne!(visible_text(&text,line).chars().last(),Some(opening),"opening bracket at line end: U+{:04X}",opening as u32);}
        }
    }
}
#[test]
fn mixed_korean_and_latin_keeps_latin_words_whole() {
    let fixture=Fixture::new();let word="affinity";
    let text=format!("\u{ac00}\u{b098} {word} \u{b2e4}\u{b77c}\u{b9c8}\u{bc14}");
    let start=text[..text.find(word).expect("Latin fixture word")].chars().count();let end=start+word.chars().count();
    let lines=fixture.layout(&text,Some(fixture.width(word)*1.1));assert!(lines.len()>1);
    assert!(lines.iter().any(|line|line.range.start<=start && line.range.end>=end),"one line must contain the complete Latin word");
    for line in &lines {
        assert!(!(start<line.range.start && line.range.start<end),"line starts inside Latin word");
        assert!(!(start<line.range.end && line.range.end<end),"line ends inside Latin word");
    }
}
#[test]
fn nbsp_and_word_joiner_prevent_breaks() {
    let fixture=Fixture::new();
    for joiner in ['\u{00a0}','\u{2060}'] {
        let text=format!("\u{ac00}{joiner}\u{b098}");let natural=fixture.width(&text);
        let control=fixture.layout("\u{ac00}\u{b098}",Some(natural*0.6));
        assert!(control.len()>1,"unjoined Hangul must wrap at this width");
        let lines=fixture.layout(&text,Some(natural*0.6));
        assert_eq!(lines.len(),1,"unbreakable scalars must overflow together: U+{:04X}",joiner as u32);
        assert_eq!(lines[0].range,TextRange::new(0,3));
    }
}
