use dtool_core::table::{commands::MAX_ROWS_COLS, merge, selection, sort};
use dtool_core::{Command, ObjectId, ObjectKind, Session};
use serde_json::json;

pub(crate) fn command_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "text.table.merge_cells" => merge::MERGE,
        "text.table.separate_cells" => merge::UNMERGE,
        "text.table.insert_row" => "table.row.insert",
        "text.table.insert_column" => "table.column.insert",
        "text.table.delete_row_s" => "table.row.delete",
        "text.table.delete_column_s" => "table.column.delete",
        "text.table.evenly_distribute_rows" => "table.rows.distribute",
        "text.table.evenly_distribute_columns" => "table.columns.distribute",
        "text.table.autofit_rows" => "table.row.autofit",
        "text.table.autofit_columns" => "table.column.autofit",
        "text.table.sort.sort_by_row_ascending" | "text.table.sort.sort_by_row_descending" => {
            sort::COLUMNS
        }
        "text.table.sort.sort_by_column_ascending"
        | "text.table.sort.sort_by_column_descending" => sort::ROWS,
        super::sort_options::MENU => sort::OPTIONS_QUERY,
        _ => return None,
    })
}

pub(crate) fn command(session: &Session, id: &str) -> Option<Command> {
    let name = command_name(id)?;
    if session.active_tool != "tool.table" {
        return None;
    }
    if matches!(name, merge::MERGE | merge::UNMERGE) {
        return Some(Command::new(name));
    }
    let [object_id] = session.selection.ids.as_slice() else {
        return None;
    };
    let object = session.document.get_object(*object_id)?;
    let ObjectKind::Table(table) = &object.kind else {
        return None;
    };
    if object.locked {
        return None;
    }
    if name == sort::OPTIONS_QUERY {
        return Some(Command::new(name));
    }
    let selection = selection::current(session);
    if matches!(name, sort::ROWS | sort::COLUMNS) {
        let rows = name == sort::ROWS;
        let key = selection
            .track_indices(table, !rows)
            .first()
            .copied()
            .unwrap_or(0);
        return Some(sort_command(
            *object_id,
            rows,
            key,
            id.ends_with("_descending"),
        ));
    }
    let rows = matches!(
        name,
        "table.row.insert" | "table.row.delete" | "table.rows.distribute" | "table.row.autofit"
    );
    let (field, list, count) = if rows {
        ("row", "rows", table.row_heights_pt.len())
    } else {
        ("col", "cols", table.col_widths_pt.len())
    };
    let indices = selection.track_indices(table, rows);
    let mut params = json!({"id": object_id.0});
    if name.ends_with(".insert") {
        if count >= MAX_ROWS_COLS as usize {
            return None;
        }
        params[field] = json!(indices.last()? + 1);
        params["select"] = json!(true);
        params["match_neighbor"] = json!(true);
    } else {
        if name.ends_with(".delete") {
            if indices.is_empty() || indices.len() >= count {
                return None;
            }
            params["select"] = json!(true);
        } else if name.ends_with(".distribute")
            && (if indices.is_empty() {
                count
            } else {
                indices.len()
            }) < 2
        {
            return None;
        }
        params[list] = json!(indices);
    }
    Some(Command::with_params(name, params))
}

pub(crate) fn sort_command(table: ObjectId, rows: bool, key: u32, descending: bool) -> Command {
    let mut params =
        json!({"table": table.0, "order": if descending { "descending" } else { "ascending" }});
    params[if rows { "column" } else { "row" }] = json!(key);
    Command::with_params(if rows { sort::ROWS } else { sort::COLUMNS }, params)
}
