use dtool_core::Document;
use dtool_core::table::model::*;
use dtool_core::table::resolve::{resolve_cell,CELL_MINIMUM};
use dtool_core::table::geometry::{table_grid,cell_at,to_local,edge_owner,EdgeSide};

fn table(rows:u32,cols:u32)->TableNode {
    let mut alloc=CellIdAllocator{high_water:0};
    let mut cells=Vec::new();
    for row in 0..rows {for col in 0..cols{cells.push(TableCell{id:alloc.next(),row,col,row_span:1,col_span:1,merged_into:None,story:StoryId(u64::from(row*cols+col)),local:CellFormat::default(),mask:CellFormatMask::default()});}}
    TableNode{alloc,col_widths_pt:vec![40.0;cols as usize],row_heights_pt:vec![20.0;rows as usize],cells,header_rows:0,footer_rows:0,format:None,frame_local:TableFrameFormat::default(),frame_mask:FrameFormatMask::default(), text_defaults: None}
}

fn definition(id:u64,name:&str,minimum:f32)->TableFormatDef {
    let mut cell=CellFormat::default();cell.minimum_pt=minimum;
    TableFormatDef{id:TableFormatId(id),name:name.into(),frame:TableFrameFormat::default(),cell_formats:vec![(0,"body".into(),cell)],pattern:TablePattern{rows:1,cols:1,top_band_rows:0,bottom_band_rows:0,left_band_cols:0,right_band_cols:0,slots:vec![0]}}
}

#[test]
fn local_then_pattern_then_document_then_factory_resolve_per_field(){
    let mut document=Document::default();let mut table=table(1,1);
    assert_eq!(resolve_cell(&document,&table,&table.cells[0]).minimum_pt,0.0);
    document.table_formats.push(definition(1,"document",3.0));document.default_table_format=Some(TableFormatId(1));
    assert_eq!(resolve_cell(&document,&table,&table.cells[0]).minimum_pt,3.0);
    let mut pattern=definition(2,"table",7.0);pattern.cell_formats[0].2.fill=dtool_core::paint::paint_from_legacy_str("#102030");
    document.table_formats.push(pattern);table.format=Some(TableFormatId(2));
    assert_eq!(resolve_cell(&document,&table,&table.cells[0]).minimum_pt,7.0);
    table.cells[0].local.minimum_pt=11.0;table.cells[0].mask=CellFormatMask(CELL_MINIMUM);
    let resolved=resolve_cell(&document,&table,&table.cells[0]);assert_eq!(resolved.minimum_pt,11.0);assert_eq!(resolved.fill_str().as_deref(),Some("#102030"));
}

#[test]
fn top_bottom_bands_keep_body_rows_on_five_row_table(){
    let mut document=Document::default();let mut table=table(5,1);let mut format=definition(1,"bands",0.0);
    format.cell_formats=(0..3).map(|index|{let mut cell=CellFormat::default();cell.minimum_pt=(index+1) as f32;(index,format!("slot{index}"),cell)}).collect();
    format.pattern=TablePattern{rows:3,cols:1,top_band_rows:1,bottom_band_rows:1,left_band_cols:0,right_band_cols:0,slots:vec![0,1,2]};
    document.table_formats.push(format);table.format=Some(TableFormatId(1));
    let values:Vec<f32>=table.cells.iter().map(|cell|resolve_cell(&document,&table,cell).minimum_pt).collect();assert_eq!(values,vec![1.0,2.0,2.0,2.0,3.0]);
}

#[test]
fn grid_three_by_three_and_two_by_two_merge_rectangles(){
    let mut table=table(3,3);let grid=table_grid(&table);
    assert_eq!(grid.col_x,vec![0.0,40.0,80.0,120.0]);assert_eq!(grid.row_y,vec![0.0,20.0,40.0,60.0]);assert_eq!(grid.rects.len(),9);
    let anchor=table.cells[0].id;table.cells[0].row_span=2;table.cells[0].col_span=2;
    for index in [1,3,4]{table.cells[index].merged_into=Some(anchor);}
    let merged=table_grid(&table);assert_eq!(merged.rects.len(),6);assert_eq!(merged.rects.iter().find(|cell|cell.id==anchor.0).unwrap().rect,[0.0,0.0,80.0,40.0]);assert_eq!(cell_at(&merged,[60.0,30.0]),Some(anchor.0));
}

#[test]
fn shared_edge_has_single_owner_and_rotated_hit_uses_inverse_transform(){
    let table=table(3,3);let grid=table_grid(&table);
    assert_eq!(edge_owner(&grid,0,0,EdgeSide::Right),edge_owner(&grid,0,1,EdgeSide::Left));
    assert_eq!(edge_owner(&grid,0,0,EdgeSide::Bottom),edge_owner(&grid,1,0,EdgeSide::Top));
    let matrix=[0.0,1.0,-1.0,0.0,100.0,50.0];
    let local=to_local(matrix,[110.0,30.0]).unwrap();assert_eq!(local,[20.0,10.0]);assert_eq!(cell_at(&grid,local),Some(table.cells[0].id.0));
    assert_eq!(cell_at(&grid,[120.0,60.0]),None);
    assert_eq!(to_local([1.0,2.0,2.0,4.0,0.0,0.0],[1.0,1.0]),None);
}
