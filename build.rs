fn main(){
    if std::env::var("STEAM_APP_ID").is_err() {
        eprintln!("cargo:warning=STEAM_APP_ID not set in environment; loading from .env file");
        // Loads from .env in the crate root by default
        let _ = dotenvy::dotenv();
    }
    if let Ok(val) = std::env::var("STEAM_APP_ID") {
        // Export to rustc as a compile-time env var
        println!("cargo:rustc-env=STEAM_APP_ID={val}");
    }

    // Re-run the build script if .env changes (handy locally)
    println!("cargo:rerun-if-changed=.env");
}