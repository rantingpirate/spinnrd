extern crate built;
//TODO: Make spinnr.sh with version# and default spinfile from template
fn main() {
    let mut bopt = built::Options::default();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    bopt.set_compiler(false)
        .set_git(true)
        .set_ci(false)
        .set_env(true)
        .set_dependencies(true)
        .set_features(true)
        .set_time(false)
        .set_cfg(true);
    built::write_built_file_with_opts(
        &bopt,
        env!("CARGO_MANIFEST_DIR"),
        [out_dir, "built.rs".to_owned()].iter().collect::<std::path::PathBuf>()
        ).expect("Failed to aquire build-time information");
}
