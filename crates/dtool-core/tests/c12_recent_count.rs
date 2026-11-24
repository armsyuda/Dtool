use dtool_core::workspace::Workspace;
use std::path::Path;

#[test]
fn recent_list_follows_recent_count_with_minimum_one() {
    let mut ws = Workspace::default();
    for name in ["a.dtool", "b.dtool", "c.dtool"] {
        ws.push_recent(Path::new(name));
    }
    assert_eq!(ws.recent().len(), 3);
    let mut prefs = dtool_core::prefs::Prefs::default();
    prefs.recent_count = 2;
    ws.apply_prefs(&prefs);
    assert_eq!(ws.recent().len(), 2);
    assert_eq!(ws.recent()[0], Path::new("c.dtool"));
    ws.push_recent(Path::new("d.dtool"));
    assert_eq!(ws.recent().len(), 2);
    ws.set_recent_count(0);
    assert_eq!(ws.recent().len(), 1);
}