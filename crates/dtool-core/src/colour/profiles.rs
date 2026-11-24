use super::ColourModel;
use crate::CommandError;
use std::{collections::BTreeMap,path::Path,sync::Arc};

#[derive(Debug,Clone)]
pub struct IccProfile {pub name:String,pub model:ColourModel,pub bytes:Arc<[u8]>}


#[derive(Debug,Clone,Default)]
pub struct ProfileStore {profiles:BTreeMap<String,IccProfile>}

impl ProfileStore {
    pub fn builtin()->Self {
        let mut store=Self::default();
        let profile=lcms2::Profile::new_srgb();
        if let Ok(bytes)=profile.icc(){let _=store.insert(Self::SRGB.into(),bytes);}
        if let Ok(profile)=lcms2::Profile::new_lab4_context(lcms2::GlobalContext::new(),lcms2::CIExyY::d50()){
            if let Ok(bytes)=profile.icc(){let _=store.insert(Self::LAB_D50.into(),bytes);}
        }
        let _=store.insert(Self::FOGRA39L.into(),FOGRA39L_ICC.to_vec());
        let _=store.load_asset(Self::SWOP_FILE,Self::SWOP_V2);
        if let Ok(profile)=lcms2::Profile::new_gray(&lcms2::CIExyY::d50(),&lcms2::ToneCurve::new(2.2)){
            if let Ok(bytes)=profile.icc(){let _=store.insert(Self::GREY_D50.into(),bytes);}
        }
        store
    }
    pub fn insert(&mut self,name:String,bytes:Vec<u8>)->Result<String,CommandError>{
        let profile=lcms2::Profile::new_icc(&bytes).map_err(|_|CommandError::new("PROFILE_MODEL_MISMATCH"))?;
        let model=match profile.color_space(){lcms2::ColorSpaceSignature::RgbData=>ColourModel::Rgb,lcms2::ColorSpaceSignature::CmykData=>ColourModel::Cmyk,lcms2::ColorSpaceSignature::LabData=>ColourModel::Lab,lcms2::ColorSpaceSignature::GrayData=>ColourModel::Grey,_=>return Err(CommandError::new("PROFILE_MODEL_MISMATCH"))};
        if name.trim().is_empty(){return Err(CommandError::invalid_params());}
        self.profiles.insert(name.clone(),IccProfile{name:name.clone(),model,bytes:bytes.into()});Ok(name)
    }
    pub fn load_system(&mut self){
        if let Some(root)=std::env::var_os("WINDIR"){
            let directory=std::path::PathBuf::from(root).join("System32/spool/drivers/color");
            if let Ok(entries)=std::fs::read_dir(directory){for entry in entries.flatten(){let path=entry.path();if path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("icc")||s.eq_ignore_ascii_case("icm")){let _=self.import(&path);}}}
        }
    }
    pub fn import(&mut self,path:&Path)->Result<String,CommandError>{
        let bytes=std::fs::read(path).map_err(|_|CommandError::new("PROFILE_NOT_FOUND"))?;
        let stem=path.file_stem().and_then(|s|s.to_str()).ok_or_else(CommandError::invalid_params)?.to_string();
        let name=profile_name(&bytes).unwrap_or(stem);
        self.insert(name,bytes)
    }
    pub fn import_with_model(&mut self,path:&Path)->Result<(String,ColourModel),CommandError>{
        let name=self.import(path)?;
        let model=self.profiles.get(&name).map(|profile|profile.model).ok_or_else(||CommandError::new("PROFILE_NOT_FOUND"))?;
        Ok((name,model))
    }
    pub const SRGB:&str="sRGB IEC61966-2.1";
    pub const LAB_D50:&str="LAB D50";
    pub const FOGRA39L:&str="FOGRA39L Coated";
    pub const SWOP_V2:&str="U.S. Web Coated (SWOP) v2";
    pub const SWOP_FILE:&str="USWebCoatedSWOP.icc";
    pub fn asset_dirs()->Vec<std::path::PathBuf>{
        let mut dirs=Vec::new();
        if let Some(dir)=std::env::var_os("DTOOL_ICC_DIR"){dirs.push(std::path::PathBuf::from(dir));}
        if let Some(dir)=std::env::current_exe().ok().and_then(|exe|exe.parent().map(|parent|parent.join("assets").join("icc"))){dirs.push(dir);}
        dirs.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("icc"));
        dirs
    }
    pub fn load_asset(&mut self,file:&str,name:&str)->Option<String>{
        Self::asset_dirs().into_iter().find_map(|dir|std::fs::read(dir.join(file)).ok().and_then(|bytes|self.insert(name.to_string(),bytes).ok()))
    }
    pub const GREY_D50:&str="Greyscale D50";
    pub fn alias(name:&str)->Option<&'static str>{
        let key=name.trim().to_ascii_lowercase();
        match key.as_str(){"srgb"|"srgb iec61966-2.1"|"srgb iec61966-2-1"|"srgb iec 61966-2.1"=>Some(Self::SRGB),"lab"|"lab d50"|"cie lab"=>Some(Self::LAB_D50),_=>None}
    }
    pub fn find(&self,name:&str)->Option<&IccProfile>{self.profiles.get(name).or_else(||Self::alias(name).and_then(|alias|self.profiles.get(alias)))}
    pub fn canonical(&self,name:&str)->String{self.find(name).map(|profile|profile.name.clone()).unwrap_or_else(||name.to_string())}
    pub fn list(&self,model:ColourModel)->Vec<&str>{self.profiles.values().filter(|p|p.model==model).map(|p|p.name.as_str()).collect()}
}

const FOGRA39L_ICC:&[u8]=include_bytes!("../../assets/icc/FOGRA39L_coated.icc");

pub fn profile_name(bytes:&[u8])->Option<String>{
    let profile=lcms2::Profile::new_icc(bytes).ok()?;
    let name=profile.info(lcms2::InfoType::Description,lcms2::Locale::new("en_US")).or_else(||profile.info(lcms2::InfoType::Description,lcms2::Locale::none()))?;
    let name=name.trim().trim_end_matches('\0').trim().to_string();
    (!name.is_empty()).then_some(name)
}

pub fn assign_profile(doc: &mut crate::document::Document, profile: &str, store: &ProfileStore) -> Result<(), CommandError> {
    let target = store.find(profile).ok_or_else(|| CommandError::new("PROFILE_NOT_FOUND"))?;
    if target.model != doc.colour.format.model { return Err(CommandError::new("PROFILE_MODEL_MISMATCH")); }
    doc.colour.profile = profile.to_string();
    Ok(())
}
