use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

fn valid_name(name: &str) -> bool {
    name.strip_suffix(".jpg").is_some_and(|stem| {
        !stem.is_empty() && stem.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
}

pub fn resolve(root: &Path, name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        bail!("Invalid photo name");
    }
    let path = root.join(name);
    if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
        bail!("Not a regular photo file");
    }
    Ok(path)
}

pub fn list(root: &Path) -> Result<Vec<String>> {
    let mut names = std::fs::read_dir(root)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_str()?.to_owned();
            (valid_name(&name) && entry.file_type().ok()?.is_file()).then_some(name)
        })
        .collect::<Vec<_>>();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names.truncate(24);
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal_and_non_photo_files() {
        for name in [
            "../secret.jpg",
            "/tmp/a.jpg",
            "a/b.jpg",
            ".jpg",
            ".env",
            "a.jpg/..",
            "%2e%2e.jpg",
        ] {
            assert!(!valid_name(name));
        }
        assert!(valid_name("20260909T030000Z-abcdef12.jpg"));
    }
    #[test]
    fn refuses_symlinks_and_orders_latest_first() {
        let dir = std::env::temp_dir().join(format!("local-photos-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"photo").unwrap();
        std::fs::write(dir.join("b.jpg"), b"photo").unwrap();
        std::os::unix::fs::symlink(dir.join("a.jpg"), dir.join("c.jpg")).unwrap();
        assert!(resolve(&dir, "c.jpg").is_err());
        assert_eq!(list(&dir).unwrap(), ["b.jpg", "a.jpg"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
