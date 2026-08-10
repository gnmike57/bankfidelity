use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    if env::var("CARGO_FEATURE_OCR").is_ok() {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let models_dir = Path::new(&manifest_dir).join("models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir).unwrap();
        }

        let detection_model = models_dir.join("text-detection.rten");
        let recognition_model = models_dir.join("text-recognition.rten");

        let detection_url =
            "https://github.com/robertknight/ocrs-models/raw/main/text-detection.rten";
        let recognition_url =
            "https://github.com/robertknight/ocrs-models/raw/main/text-recognition.rten";

        download_file(detection_url, &detection_model);
        download_file(recognition_url, &recognition_model);
    }
}

fn download_file(url: &str, dest: &Path) {
    if !dest.exists() {
        println!("cargo:warning=Downloading {} to {}...", url, dest.display());
        let status = if cfg!(windows) {
            Command::new("curl.exe")
                .arg("-L")
                .arg("-o")
                .arg(dest)
                .arg(url)
                .status()
                .expect("Failed to execute curl.exe")
        } else {
            Command::new("curl")
                .arg("-L")
                .arg("-o")
                .arg(dest)
                .arg(url)
                .status()
                .expect("Failed to execute curl")
        };

        if !status.success() {
            panic!("Failed to download {}", url);
        }
    }
}
