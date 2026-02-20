use std::{collections::HashMap, path::PathBuf};
 
fn main() {
    let library = HashMap::from([(
        "lucide".to_string(),
        PathBuf::from(lucide_slint::get_slint_file_path().to_string()),
    )]);
    let config = slint_build::CompilerConfiguration::new().with_library_paths(library);

    // Specify your Slint code entry here
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint build failed");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/rv_icon.ico");
        res.compile().expect("Windows resource build failed");
    }
}
