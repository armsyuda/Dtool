use crate::CommandError;
use std::collections::HashSet;

pub fn parse_page_ranges(text: &str, total: usize) -> Result<Vec<usize>, CommandError> {
    let mut pages = HashSet::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.contains('–') || part.contains('-') {
            let (start_str, end_str) = part.split_once(['–', '-']).unwrap();
            let start: usize = start_str.trim().parse().map_err(|_| CommandError::invalid_params())?;
            let end: usize = end_str.trim().parse().map_err(|_| CommandError::invalid_params())?;
            if start > end {
                return Err(CommandError::invalid_params());
            }
            if start < 1 || end > total {
                return Err(CommandError::invalid_params());
            }
            for p in start..=end {
                pages.insert(p);
            }
        } else {
            let p: usize = part.parse().map_err(|_| CommandError::invalid_params())?;
            if p < 1 || p > total {
                return Err(CommandError::invalid_params());
            }
            pages.insert(p);
        }
    }
    let mut result: Vec<usize> = pages.into_iter().collect();
    result.sort();
    Ok(result)
}

pub fn unique_name(existing: &[String], base: &str) -> String {
    if !existing.iter().any(|s| s == base) {
        return base.to_string();
    }
    let mut counter = 2;
    loop {
        let candidate = format!("{} {}", base, counter);
        if !existing.iter().any(|s| s == &candidate) {
            return candidate;
        }
        counter += 1;
    }
}

#[derive(Debug,Clone,serde::Serialize)]
pub struct RecentIds {ids:Vec<u64>,pub cap:usize}

impl Default for RecentIds {fn default()->Self{Self::new(10)}}

impl<'de> serde::Deserialize<'de> for RecentIds {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
        #[derive(serde::Deserialize)]
        struct Wire {#[serde(default)]ids:Vec<u64>,#[serde(default="default_cap",alias="max_size")]cap:usize}
        fn default_cap()->usize{10}
        let value=Wire::deserialize(deserializer)?;let mut recent=Self::new(value.cap);
        for id in value.ids.into_iter().rev(){recent.push(id);}Ok(recent)
    }
}

impl RecentIds {
    pub fn new(max_size:usize)->Self {Self{ids:Vec::new(),cap:max_size.min(10)}}
    pub fn push(&mut self,id:u64){self.ids.retain(|old|*old!=id);self.ids.insert(0,id);self.ids.truncate(self.cap.min(10));}
    pub fn add(&mut self,id:u64){self.push(id);}
    pub fn contains(&self,id:u64)->bool{self.ids.contains(&id)}
    pub fn list(&self)->&[u64]{&self.ids}
}
