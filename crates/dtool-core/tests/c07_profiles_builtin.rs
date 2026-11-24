use dtool_core::colour::profiles::{profile_name, ProfileStore};
use dtool_core::colour::ColourModel;

#[test]
fn c07_builtin_has_fogra39l_cmyk_and_grey_d50() {
    let store = ProfileStore::builtin();
    let fogra = store.find(ProfileStore::FOGRA39L).expect("FOGRA39L Coated");
    assert_eq!(fogra.model, ColourModel::Cmyk);
    assert_eq!(store.find(ProfileStore::GREY_D50).expect("grey").model, ColourModel::Grey);
    assert!(store.list(ColourModel::Cmyk).contains(&ProfileStore::FOGRA39L));

}

#[test]
fn c07_mid_grey_round_trips_through_fogra39l() {
    let store = ProfileStore::builtin();
    let srgb = lcms2::Profile::new_icc(&store.find(ProfileStore::SRGB).unwrap().bytes).unwrap();
    let cmyk = lcms2::Profile::new_icc(&store.find(ProfileStore::FOGRA39L).unwrap().bytes).unwrap();
    let to_cmyk = lcms2::Transform::new(&srgb, lcms2::PixelFormat::RGB_8, &cmyk, lcms2::PixelFormat::CMYK_8, lcms2::Intent::RelativeColorimetric).unwrap();
    let to_rgb = lcms2::Transform::new(&cmyk, lcms2::PixelFormat::CMYK_8, &srgb, lcms2::PixelFormat::RGB_8, lcms2::Intent::RelativeColorimetric).unwrap();
    let rgb_in = [[128u8, 128, 128]];
    let mut ink = [[0u8; 4]];
    to_cmyk.transform_pixels(&rgb_in, &mut ink);
    let mut rgb_out = [[0u8; 3]];
    to_rgb.transform_pixels(&ink, &mut rgb_out);
    let lab = lcms2::Profile::new_lab4_context(lcms2::GlobalContext::new(), lcms2::CIExyY::d50()).unwrap();
    let to_lab = lcms2::Transform::new(&srgb, lcms2::PixelFormat::RGB_8, &lab, lcms2::PixelFormat::Lab_DBL, lcms2::Intent::RelativeColorimetric).unwrap();
    let mut a = [[0f64; 3]];
    let mut b = [[0f64; 3]];
    to_lab.transform_pixels(&rgb_in, &mut a);
    to_lab.transform_pixels(&rgb_out, &mut b);
    let de = (0..3).map(|i| (a[0][i] - b[0][i]).powi(2)).sum::<f64>().sqrt();
    assert!(de < 2.0, "dE {de} {rgb_out:?}");
}

#[test]
fn c07_import_names_profile_by_desc_tag() {
    let srgb = lcms2::Profile::new_srgb().icc().unwrap();
    let desc = profile_name(&srgb).expect("desc");
    let dir = std::env::temp_dir().join(format!("c07_profile_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("some_file_stem.icc");
    std::fs::write(&path, &srgb).unwrap();
    let mut store = ProfileStore::default();
    let (name, model) = store.import_with_model(&path).expect("import");
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
    assert_eq!(name, desc);
    assert_ne!(name, "some_file_stem");
    assert_eq!(model, ColourModel::Rgb);
    assert_eq!(profile_name(super_fogra()).as_deref().map(|name| !name.is_empty()), Some(true));
}

fn super_fogra() -> &'static [u8] {
    include_bytes!("../assets/icc/FOGRA39L_coated.icc")
}