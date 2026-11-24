use crate::{Command,CommandError,CommandResult,Session};
use crate::colour::{Colour,ColourModel,ConvertCtx};

pub fn can_execute(session:&Session,command:&Command)->Option<bool>{(command.name=="color.convert").then_some(session.has_document)}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult>{
    if command.name!="color.convert"{return None;}
    Some((||{
        let colour:Colour=serde_json::from_value(command.params.get("colour").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
        let model:ColourModel=serde_json::from_value(command.params.get("to").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
        let services=crate::colour::services().read().map_err(|_|CommandError::new("NOT_AVAILABLE"))?;
        let prefs=&services.preferences;
        let target=match model{ColourModel::Rgb=>&prefs.rgb,ColourModel::Cmyk=>&prefs.cmyk,ColourModel::Lab=>&prefs.lab,ColourModel::Grey=>&prefs.grey};
        let ctx=ConvertCtx{from:session.document.colour.profile.clone(),to:target.clone(),intent:prefs.intent,bpc:prefs.bpc};
        let result=crate::colour::convert(&colour,model,&ctx,&services.profiles)?;
        serde_json::to_value(result).map_err(|_|CommandError::invalid_params())
    })())
}
