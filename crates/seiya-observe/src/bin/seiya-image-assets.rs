//! Manual operator utility. Verify existing assets without transferring again.
#[tokio::main]
async fn main() -> Result<(), String> {
    use seiya_observe::image::assets::Selection;
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, path] if mode == "--verify" => {
            for a in Selection::selected().map_err(str::to_string)?.assets {
                seiya_observe::image::assets::verify(
                    &std::path::Path::new(path).join(&a.local_name),
                    &a,
                )?;
                println!("verified {} {} {}", a.local_name, a.bytes, a.sha256);
            }
            Ok(())
        }
        #[cfg(target_os = "macos")]
        [mode, path, flag, approval]
            if mode == "--download"
                && flag == "--approval"
                && approval == "image21-turbo-q4-three-assets-noncommercial-evaluation" =>
        {
            seiya_observe::image::download::download(std::path::Path::new(path)).await
        }
        _ => Err("use --verify DIRECTORY; new download needs explicit operator approval".into()),
    }
}
