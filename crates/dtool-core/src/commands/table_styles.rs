use crate::{Command, CommandResult, Session};

pub fn can_execute(session:&Session,command:&Command)->Option<bool>{
    use crate::table::styles::{DEFAULT_RESET, DEFAULT_SAVE};
    if command.name == DEFAULT_SAVE {
        return Some(session.has_document && !session.document.table_formats.is_empty());
    }
    if command.name == DEFAULT_RESET {
        return Some(session.has_document);
    }
    matches!(command.name.as_str(),"style.table.define"|"style.table.list"|"style.table.query"|"style.table.duplicate"|"style.table.delete"|"style.table.apply"|"style.table.from_selection"|"style.table.import"|"style.table.import.query").then_some(session.has_document)
}

pub fn run(session:&mut Session,command:&Command)->Option<CommandResult>{
    crate::table::styles::run(session,command)
}

#[cfg(test)]
mod live_route_contracts {
    use super::*;

    #[test]
    fn style_table_route_is_live_and_gated_on_document() {
        let mut session = Session::new(crate::Document::default());
        let list = Command::new("style.table.list");
        assert_eq!(crate::commands::can_execute(&session, &list), Some(true));
        assert!(crate::commands::run(&mut session, &list).is_some_and(|r| r.is_ok()));
        assert_eq!(crate::commands::can_execute(&session, &Command::new("style.table.delete")), Some(true));
        session.has_document = false;
        assert_eq!(crate::commands::can_execute(&session, &list), Some(false));
        assert_eq!(crate::commands::can_execute(&session, &Command::new("table.create")), None);
    }
}
