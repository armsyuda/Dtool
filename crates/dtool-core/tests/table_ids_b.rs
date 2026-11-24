use dtool_core::table::ids::{CellId, CellIdAllocator};
use dtool_protocol::CommandError;

#[test]
fn allocator_shared_across_create_insert_row_insert_column() {
    let mut alloc = CellIdAllocator { high_water: 0 };

    let id1 = alloc.next();
    assert_eq!(id1.0, 1);

    let id2 = alloc.next();
    assert_eq!(id2.0, 2);

    let id3 = alloc.next();
    assert_eq!(id3.0, 3);

    assert_eq!(alloc.high_water, 3);
}

#[test]
fn deleting_max_id_does_not_reuse_it() {
    let mut alloc = CellIdAllocator { high_water: 10 };

    let id = alloc.next();
    assert_eq!(id.0, 11);
    assert_eq!(alloc.high_water, 11);

    let next = alloc.next();
    assert_eq!(next.0, 12);
    assert_eq!(alloc.high_water, 12);
}

#[test]
fn parse_legacy_rejects_bad_text() {
    assert!(CellIdAllocator::parse_legacy("").is_none());
    assert!(CellIdAllocator::parse_legacy("c").is_none());
    assert!(CellIdAllocator::parse_legacy("cell5").is_none());
    assert!(CellIdAllocator::parse_legacy("c-1").is_none());
    assert!(CellIdAllocator::parse_legacy("cabc").is_none());

    let valid = CellIdAllocator::parse_legacy("c42");
    assert_eq!(valid, Some(CellId(42)));
}

#[test]
fn hydrate_rejects_duplicate_ids() {
    let ids = vec![CellId(1), CellId(2), CellId(1)];
    let result = CellIdAllocator::hydrate(&ids);
    assert!(result.is_err());
}

#[test]
fn hydrate_sets_high_water_to_max() {
    let ids = vec![CellId(5), CellId(3), CellId(8)];
    let alloc = CellIdAllocator::hydrate(&ids).unwrap();
    assert_eq!(alloc.high_water, 8);
}

#[test]
fn hydrate_empty_is_valid() {
    let alloc = CellIdAllocator::hydrate(&[]).unwrap();
    assert_eq!(alloc.high_water, 0);
}
