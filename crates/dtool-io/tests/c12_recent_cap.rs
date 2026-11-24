use dtool_io::RecentList;
use std::path::PathBuf;

#[test]
fn c12_recent_list_honours_cap() {
    let dir = PathBuf::from("c12_recent_cap");
    let mut list = RecentList::with_cap(2);
    for name in ["a", "b", "c"] {
        list.push(&dir.join(format!("{name}.dtool")));
    }
    assert_eq!(list.paths(), &[dir.join("c.dtool"), dir.join("b.dtool")]);
    list.set_cap(0);
    assert_eq!(list.cap(), 1);
    assert_eq!(list.paths(), &[dir.join("c.dtool")]);
    assert_eq!(RecentList::new().cap(), 10);
}
