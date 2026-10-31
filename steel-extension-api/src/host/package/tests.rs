use super::*;
use std::error::Error;
use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};

fn archive(manifest: &str, member: &str) -> Result<(TempDir, PathBuf), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("extension.zip");
    let mut zip = ZipWriter::new(File::create(&path)?);
    zip.start_file("extension.toml", SimpleFileOptions::default())?;
    zip.write_all(manifest.as_bytes())?;
    zip.start_file(member, SimpleFileOptions::default())?;
    zip.write_all(b"native library fixture")?;
    zip.finish()?;
    Ok((directory, path))
}
fn manifest(target: &str, member: &str) -> String {
    format!("[package]\nname = 'fixture'\nversion = '1'\n[targets]\n{target} = '{member}'\n")
}

#[test]
fn selects_current_platform_and_extracts_only_its_library() -> Result<(), Box<dyn Error>> {
    let (_source, path) = archive(
        &manifest(&format!("{OS}-{ARCH}"), "native/library.bin"),
        "native/library.bin",
    )?;
    let prepared = prepare(&path)?;
    assert_eq!(prepared.name, "fixture");
    assert_eq!(fs::read(&prepared.library)?, b"native library fixture");
    assert_eq!(fs::read_dir(prepared.directory.path())?.count(), 1);
    Ok(())
}

#[test]
fn rejects_unsupported_platforms_and_traversal_before_extraction() -> Result<(), Box<dyn Error>> {
    let (_source, path) = archive(&manifest("other-platform", "library.bin"), "library.bin")?;
    assert!(
        prepare(&path)
            .err()
            .is_some_and(|e| e.contains("does not support"))
    );
    for member in [
        "../escape.bin",
        "/escape.bin",
        "native/../escape.bin",
        "native\\escape.bin",
    ] {
        let (_source, path) = archive(&manifest(&format!("{OS}-{ARCH}"), member), "unused.bin")?;
        assert!(
            prepare(&path)
                .err()
                .is_some_and(|e| e.contains("relative, enclosed"))
        );
    }
    Ok(())
}

#[test]
fn archive_discovery_is_stable_and_excludes_directories() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    File::create(directory.path().join("b.zip"))?;
    File::create(directory.path().join("a.zip"))?;
    File::create(directory.path().join("readme.txt"))?;
    fs::create_dir(directory.path().join("directory.zip"))?;
    assert_eq!(
        discover_archives(directory.path())?,
        vec![
            directory.path().join("a.zip"),
            directory.path().join("b.zip")
        ]
    );
    assert_eq!(
        discover_archives(&directory.path().join("missing"))?,
        Vec::<PathBuf>::new()
    );
    Ok(())
}
