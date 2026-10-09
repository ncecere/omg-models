fn main() {
    // styles.css (installed by `topcoat ui init`, then mapped onto the OMG
    // brand tokens) is the Tailwind input. Topcoat downloads its pinned
    // Tailwind CLI release once and caches it under the target directory;
    // set TAILWIND_CLI to use a preinstalled binary instead.
    let mut config = topcoat::tailwind::BuildConfig::new().input("styles.css");
    if std::env::var_os("TAILWIND_CLI").is_some() {
        config = config.executable_env("TAILWIND_CLI");
    }
    println!("cargo:rerun-if-env-changed=TAILWIND_CLI");
    println!("cargo:rerun-if-changed=styles.css");
    println!("cargo:rerun-if-changed=brand.css");
    println!("cargo:rerun-if-changed=src");
    config.render().unwrap();
}
