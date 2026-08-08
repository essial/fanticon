use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, BufReader, Read},
    path::Path,
};

const MANIFEST_NAME: &str = ".fanticon-code-assets";

pub fn sync_children(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("source directory does not exist: {}", source.display()),
        ));
    }
    fs::create_dir_all(destination)?;

    let manifest_path = destination.join(MANIFEST_NAME);
    let previous = fs::read_to_string(&manifest_path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let mut current = HashSet::<String>::new();

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "code asset name is not valid UTF-8")
        })?;
        validate_managed_name(&name)?;
        current.insert(name.clone());
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("code assets cannot contain symlinks: {}", source_path.display()),
            ));
        }
        if file_type.is_dir() {
            sync_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            if destination_path.is_dir() {
                fs::remove_dir_all(&destination_path)?;
            }
            if !files_equal(&source_path, &destination_path)? {
                fs::copy(source_path, destination_path)?;
            }
        }
    }

    for stale in previous.difference(&current) {
        validate_managed_name(stale)?;
        let path = destination.join(stale);
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
    }

    let mut names = current.into_iter().collect::<Vec<_>>();
    names.sort();
    fs::write(manifest_path, names.join("\n"))
}

fn validate_managed_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\n', '\r']) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid managed asset name"));
    }
    Ok(())
}

pub fn sync_tree(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("source directory does not exist: {}", source.display()),
        ));
    }
    if destination.exists() && !destination.is_dir() {
        fs::remove_file(destination)?;
    }
    fs::create_dir_all(destination)?;
    sync_directory(source, destination)
}

fn sync_directory(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source_names = HashSet::<OsString>::new();

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        source_names.insert(name.clone());
        let source_path = entry.path();
        let destination_path = destination.join(name);
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("code assets cannot contain symlinks: {}", source_path.display()),
            ));
        }
        if file_type.is_dir() {
            if destination_path.exists() && !destination_path.is_dir() {
                fs::remove_file(&destination_path)?;
            }
            fs::create_dir_all(&destination_path)?;
            sync_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            if destination_path.is_dir() {
                fs::remove_dir_all(&destination_path)?;
            }
            if !files_equal(&source_path, &destination_path)? {
                fs::copy(source_path, destination_path)?;
            }
        }
    }

    for entry in fs::read_dir(destination)? {
        let entry = entry?;
        if source_names.contains(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> io::Result<bool> {
    let Ok(right_metadata) = fs::metadata(right) else {
        return Ok(false);
    };
    let left_metadata = fs::metadata(left)?;
    if !right_metadata.is_file() || left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }

    let mut left = BufReader::new(fs::File::open(left)?);
    let mut right = BufReader::new(fs::File::open(right)?);
    let mut left_buffer = [0; 8192];
    let mut right_buffer = [0; 8192];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sync_copies_updates_and_removes_stale_entries() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("fanticon-code-assets-{nonce}"));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("demos/wave")).unwrap();
        fs::create_dir_all(destination.join("obsolete")).unwrap();
        fs::write(source.join("demos/wave/main.asm"), "FIRST").unwrap();
        fs::write(destination.join("stale.txt"), "STALE").unwrap();

        sync_tree(&source, &destination).unwrap();
        assert_eq!(fs::read_to_string(destination.join("demos/wave/main.asm")).unwrap(), "FIRST");
        assert!(!destination.join("stale.txt").exists());
        assert!(!destination.join("obsolete").exists());

        fs::write(source.join("demos/wave/main.asm"), "SECOND").unwrap();
        sync_tree(&source, &destination).unwrap();
        assert_eq!(fs::read_to_string(destination.join("demos/wave/main.asm")).unwrap(), "SECOND");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn child_sync_preserves_user_content_and_removes_only_stale_managed_children() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("fanticon-code-children-{nonce}"));
        let source = root.join("source");
        let destination = root.join("fanticon");
        fs::create_dir_all(source.join("demos")).unwrap();
        fs::create_dir_all(destination.join("music")).unwrap();
        fs::write(source.join("demos/readme.txt"), "DEMOS").unwrap();
        fs::write(source.join("fanticon.inc"), "BANK_KIND EQU $C000").unwrap();
        fs::write(destination.join("music/song.nsf"), "MUSIC").unwrap();

        sync_children(&source, &destination).unwrap();
        assert_eq!(fs::read_to_string(destination.join("demos/readme.txt")).unwrap(), "DEMOS");
        assert_eq!(
            fs::read_to_string(destination.join("fanticon.inc")).unwrap(),
            "BANK_KIND EQU $C000"
        );
        assert_eq!(fs::read_to_string(destination.join("music/song.nsf")).unwrap(), "MUSIC");

        fs::remove_dir_all(source.join("demos")).unwrap();
        fs::create_dir_all(source.join("examples")).unwrap();
        sync_children(&source, &destination).unwrap();
        assert!(!destination.join("demos").exists());
        assert!(destination.join("examples").is_dir());
        assert!(destination.join("music/song.nsf").is_file());

        fs::remove_dir_all(root).unwrap();
    }
}
