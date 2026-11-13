#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), String> {
    use seiya_observe::image::runner;
    use std::path::PathBuf;
    let mut args = std::env::args().skip(1);
    let first = args.next().ok_or("mode_or_output_required")?;
    if first == "--validate-existing" {
        let root = PathBuf::from(args.next().ok_or("evidence_path")?);
        if args.next().is_some() {
            return Err("unexpected_arguments".into());
        }
        println!("{}", runner::validate_existing(&root)?);
        return Ok(());
    }
    if first == "--sandbox-probe" {
        let a = PathBuf::from(args.next().ok_or("probe_path")?);
        let b = PathBuf::from(args.next().ok_or("probe_alias")?);
        return runner::sandbox_probe(&a, &b);
    }
    if first == "--rehearsal" {
        let root = PathBuf::from(args.next().ok_or("rehearsal_path")?);
        if args.next().is_some() {
            return Err("unexpected_arguments".into());
        }
        println!("{}", runner::rehearsal(&root)?);
        return Ok(());
    }
    if first != "--output" {
        return Err("output_required".into());
    }
    let root = PathBuf::from(args.next().ok_or("output_path")?);
    let mut binary = None;
    let mut assets = None;
    let mut approval = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("missing_value")?;
        match flag.as_str() {
            "--binary" if binary.is_none() => binary = Some(PathBuf::from(value)),
            "--assets" if assets.is_none() => assets = Some(PathBuf::from(value)),
            "--approval" if approval.is_none() => approval = Some(value),
            _ => return Err("unexpected_argument".into()),
        }
    }
    if approval.as_deref() != Some(runner::APPROVAL) {
        return Err("explicit_one_image_approval_required".into());
    }
    let result = runner::image(
        &root,
        &binary.ok_or("binary_required")?,
        &assets.ok_or("assets_required")?,
    )
    .await?;
    let mut brief = result;
    brief
        .as_object_mut()
        .ok_or("evidence_shape")?
        .remove("samples");
    println!("{brief}");
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() -> Result<(), String> {
    Err("native_image_evaluation_unsupported_on_this_platform".into())
}
