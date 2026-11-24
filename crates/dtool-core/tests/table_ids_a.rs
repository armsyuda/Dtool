use dtool_core::table::ids::{CellId, CellIdAllocator};
use dtool_protocol::CommandError;

#[test]
fn legacy_hydrate() {
    let ids = vec![CellId(1), CellId(5), CellId(3)];
    let alloc = CellIdAllocator::hydrate(&ids).unwrap();
    assert_eq!(alloc.high_water, 5);
}

#[test]
fn duplicate_conflict() {
    let ids = vec![CellId(1), CellId(2), CellId(1)];
    let result = CellIdAllocator::hydrate(&ids);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, "TableIdConflict");
}

#[test]
fn overflow_error() {
    let mut alloc = CellIdAllocator { high_water: u64::MAX - 1 };
    let result = alloc.next();
    assert_eq!(result, CellId(u64::MAX));
    assert_eq!(alloc.high_water, u64::MAX);
    let err = CommandError::out_of_range();
    assert_eq!(err.code, "OUT_OF_RANGE");
}

#[test]
fn query_read_never_allocates() {
    let alloc = CellIdAllocator { high_water: 10 };
    let _ = alloc.high_water;
    assert_eq!(alloc.high_water, 10);
}
