#[cfg(any(target_arch = "wasm32", test))]
use std::collections::{BTreeMap, BTreeSet};

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::{cell::RefCell, rc::Rc};

pub type SharedFilesystem = Rc<RefCell<ConsoleFilesystem>>;

pub fn shared_filesystem() -> SharedFilesystem {
    Rc::new(RefCell::new(ConsoleFilesystem::new()))
}

#[derive(Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub is_directory: bool,
}

enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Native { root: PathBuf },
    #[cfg(any(target_arch = "wasm32", test))]
    Memory { directories: BTreeSet<Vec<String>>, files: BTreeMap<Vec<String>, Vec<u8>> },
    #[cfg(all(not(target_arch = "wasm32"), not(test)))]
    Unavailable(String),
}

pub struct ConsoleFilesystem {
    backend: Backend,
    cwd: Vec<String>,
}

impl ConsoleFilesystem {
    pub fn new() -> Self {
        #[cfg(any(target_arch = "wasm32", test))]
        {
            Self::memory()
        }

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            match documents_root().and_then(Self::native) {
                Ok(filesystem) => filesystem,
                Err(error) => Self { backend: Backend::Unavailable(error), cwd: Vec::new() },
            }
        }
    }

    pub fn current_directory(&self) -> String {
        if self.cwd.is_empty() { "/".to_owned() } else { format!("/{}", self.cwd.join("/")) }
    }

    /// Whether `path` resolves to `name` directly under the console root.
    pub fn is_root_file(&self, path: &str, name: &str) -> bool {
        self.normalize(path).is_ok_and(|components| {
            components.len() == 1 && components[0].eq_ignore_ascii_case(name)
        })
    }

    pub fn change_directory(&mut self, path: &str) -> Result<(), String> {
        let requested = self.normalize(path)?;
        match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let resolved = canonical_directory(root, &requested)?;
                self.cwd = relative_components(root, &resolved)?;
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, .. } => {
                if !directories.contains(&requested) {
                    return Err("Directory not found".to_owned());
                }
                self.cwd = requested;
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => return Err(error.clone()),
        }
        Ok(())
    }

    pub fn create_directory(&mut self, path: &str) -> Result<(), String> {
        let requested = self.normalize_non_root(path)?;
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let parent = requested[..requested.len() - 1].to_vec();
                let parent = canonical_directory(root, &parent)?;
                let name = requested.last().expect("non-root path");
                if case_insensitive_child(&parent, name)?.is_some() {
                    return Err("Directory already exists".to_owned());
                }
                let target = parent.join(name);
                std::fs::create_dir(&target).map_err(|error| io_error("mkdir", error))?;
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, .. } => {
                let parent = requested[..requested.len() - 1].to_vec();
                if !directories.contains(&parent) {
                    return Err("Parent directory not found".to_owned());
                }
                if !directories.insert(requested) {
                    return Err("Directory already exists".to_owned());
                }
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => return Err(error.clone()),
        }
        Ok(())
    }

    pub fn remove_directory(&mut self, path: &str) -> Result<(), String> {
        let requested = self.normalize_non_root(path)?;
        if self.cwd.starts_with(&requested) {
            return Err("Directory is in use".to_owned());
        }
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let target = canonical_directory(root, &requested)?;
                let target_relative = relative_components(root, &target)?;
                if target_relative.is_empty() {
                    return Err("Root is protected".to_owned());
                }
                if self.cwd.starts_with(&target_relative) {
                    return Err("Directory is in use".to_owned());
                }
                std::fs::remove_dir(target).map_err(|error| io_error("rmdir", error))?;
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, files } => {
                if directories
                    .iter()
                    .any(|entry| entry.len() > requested.len() && entry.starts_with(&requested))
                {
                    return Err("Directory not empty".to_owned());
                }
                if files.keys().any(|entry| entry.starts_with(&requested)) {
                    return Err("Directory not empty".to_owned());
                }
                if !directories.remove(&requested) {
                    return Err("Directory not found".to_owned());
                }
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => return Err(error.clone()),
        }
        Ok(())
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), String> {
        let requested = self.normalize_non_root(path)?;
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let parent = canonical_directory(root, &requested[..requested.len() - 1])?;
                let name = requested.last().expect("non-root file path");
                let target = case_insensitive_child(&parent, name)?
                    .ok_or_else(|| "File not found".to_owned())?;
                if !target.is_file() {
                    return Err("Not a file".to_owned());
                }
                std::fs::remove_file(target).map_err(|error| io_error("delete", error))?;
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, files } => {
                if directories.contains(&requested) {
                    return Err("Not a file".to_owned());
                }
                if files.remove(&requested).is_none() {
                    return Err("File not found".to_owned());
                }
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => return Err(error.clone()),
        }
        Ok(())
    }

    pub fn list(&self, path: Option<&str>) -> Result<Vec<DirectoryEntry>, String> {
        let requested = self.normalize(path.unwrap_or("."))?;
        match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let directory = canonical_directory(root, &requested)?;
                let mut entries = std::fs::read_dir(directory)
                    .map_err(|error| io_error("dir", error))?
                    .map(|entry| {
                        let entry = entry.map_err(|error| io_error("dir", error))?;
                        let file_type =
                            entry.file_type().map_err(|error| io_error("dir", error))?;
                        Ok(DirectoryEntry {
                            name: entry.file_name().to_string_lossy().into_owned(),
                            is_directory: file_type.is_dir(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                entries.retain(|entry| validate_name(&entry.name).is_ok());
                entries.sort_by_key(|entry| entry.name.to_ascii_uppercase());
                Ok(entries)
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, files } => {
                if !directories.contains(&requested) {
                    return Err("Directory not found".to_owned());
                }
                let mut entries = directories
                    .iter()
                    .filter(|entry| {
                        entry.len() == requested.len() + 1 && entry.starts_with(&requested)
                    })
                    .map(|entry| DirectoryEntry {
                        name: entry.last().expect("child path").clone(),
                        is_directory: true,
                    })
                    .collect::<Vec<_>>();
                entries.extend(
                    files
                        .keys()
                        .filter(|entry| {
                            entry.len() == requested.len() + 1 && entry.starts_with(&requested)
                        })
                        .map(|entry| DirectoryEntry {
                            name: entry.last().expect("child path").clone(),
                            is_directory: false,
                        }),
                );
                entries.sort_by_key(|entry| entry.name.to_ascii_uppercase());
                Ok(entries)
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => Err(error.clone()),
        }
    }

    pub fn read_text(&self, path: &str) -> Result<String, String> {
        String::from_utf8(self.read_binary(path)?).map_err(|_| "Text file is not UTF-8".to_owned())
    }

    pub fn read_binary(&self, path: &str) -> Result<Vec<u8>, String> {
        let requested = self.normalize_non_root(path)?;
        match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let path = canonical_path(root, &requested)?;
                if !path.is_file() {
                    return Err("Not a file".to_owned());
                }
                std::fs::read(path).map_err(|error| io_error("open", error))
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { files, .. } => {
                files.get(&requested).cloned().ok_or_else(|| "File not found".to_owned())
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => Err(error.clone()),
        }
    }

    /// Resolve a console path for a host-side tool while retaining the same
    /// sandbox and 8.3 validation. The final component may not exist yet.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn host_path(&self, path: &str) -> Result<PathBuf, String> {
        let requested = self.normalize_non_root(path)?;
        let Backend::Native { root } = &self.backend else {
            return Err("Host export paths are unavailable".to_owned());
        };
        let parent = canonical_directory(root, &requested[..requested.len() - 1])?;
        let name = requested.last().expect("non-root path");
        let resolved = case_insensitive_child(&parent, name)?.unwrap_or_else(|| parent.join(name));
        if !resolved.starts_with(root) {
            return Err("Cannot leave root".to_owned());
        }
        Ok(resolved)
    }

    pub fn write_text(&mut self, path: &str, text: &str) -> Result<(), String> {
        self.write_binary(path, text.as_bytes())
    }

    pub fn write_binary(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let requested = self.normalize_non_root(path)?;
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let parent = canonical_directory(root, &requested[..requested.len() - 1])?;
                let name = requested.last().expect("non-root file path");
                let path = match case_insensitive_child(&parent, name)? {
                    Some(path) => {
                        let path = path.canonicalize().map_err(|error| io_error("save", error))?;
                        if !path.starts_with(root) {
                            return Err("Cannot leave root".to_owned());
                        }
                        if !path.is_file() {
                            return Err("Not a file".to_owned());
                        }
                        path
                    }
                    None => parent.join(name),
                };
                std::fs::write(path, bytes).map_err(|error| io_error("save", error))
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, files } => {
                let parent = requested[..requested.len() - 1].to_vec();
                if !directories.contains(&parent) {
                    return Err("Parent directory not found".to_owned());
                }
                if directories.contains(&requested) {
                    return Err("Not a file".to_owned());
                }
                files.insert(requested, bytes.to_vec());
                Ok(())
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => Err(error.clone()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_binary_atomic(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let requested = self.normalize_non_root(path)?;
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Native { root } => {
                let parent = canonical_directory(root, &requested[..requested.len() - 1])?;
                let name = requested.last().expect("non-root file path");
                let destination =
                    case_insensitive_child(&parent, name)?.unwrap_or_else(|| parent.join(name));
                let temporary = parent.join(format!(".{name}.tmp"));
                {
                    use std::io::Write;
                    let mut file = std::fs::File::create(&temporary)
                        .map_err(|error| io_error("save", error))?;
                    file.write_all(bytes).map_err(|error| io_error("save", error))?;
                    file.sync_all().map_err(|error| io_error("save", error))?;
                }
                std::fs::rename(&temporary, destination).map_err(|error| io_error("save", error))
            }
            #[cfg(any(target_arch = "wasm32", test))]
            Backend::Memory { directories, files } => {
                let parent = requested[..requested.len() - 1].to_vec();
                if !directories.contains(&parent) {
                    return Err("Parent directory not found".to_owned());
                }
                files.insert(requested, bytes.to_vec());
                Ok(())
            }
            #[cfg(all(not(target_arch = "wasm32"), not(test)))]
            Backend::Unavailable(error) => Err(error.clone()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn acquire_save_lock(&self, path: &str) -> Result<Option<std::fs::File>, String> {
        use fs2::FileExt;
        let requested = self.normalize_non_root(path)?;
        let Backend::Native { root } = &self.backend else {
            return Ok(None);
        };
        let parent = canonical_directory(root, &requested[..requested.len() - 1])?;
        let name = requested.last().expect("non-root save path");
        let lock_path = parent.join(format!(".{name}.lock"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|error| io_error("save lock", error))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(io_error("save lock", error)),
        }
    }

    fn normalize(&self, path: &str) -> Result<Vec<String>, String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("Path required".to_owned());
        }
        let mut components =
            if path.starts_with(['/', '\\']) { Vec::new() } else { self.cwd.clone() };
        for component in path.split(['/', '\\']) {
            match component {
                "" | "." => {}
                ".." => {
                    if components.pop().is_none() {
                        return Err("Cannot leave root".to_owned());
                    }
                }
                component => {
                    validate_name(component)?;
                    components.push(component.to_ascii_lowercase());
                }
            }
        }
        Ok(components)
    }

    fn normalize_non_root(&self, path: &str) -> Result<Vec<String>, String> {
        let path = self.normalize(path)?;
        if path.is_empty() { Err("Root is protected".to_owned()) } else { Ok(path) }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn memory() -> Self {
        Self {
            backend: Backend::Memory {
                directories: BTreeSet::from([Vec::new()]),
                files: BTreeMap::new(),
            },
            cwd: Vec::new(),
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn native_for_test(root: &Path) -> Result<Self, String> {
        Self::native(root.to_owned())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&root).map_err(|error| io_error("create root", error))?;
        let root = root.canonicalize().map_err(|error| io_error("open root", error))?;
        Ok(Self { backend: Backend::Native { root }, cwd: Vec::new() })
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.chars().any(char::is_whitespace) {
        return Err("Spaces not allowed in names".to_owned());
    }

    let mut parts = name.split('.');
    let stem = parts.next().unwrap_or_default();
    let extension = parts.next();
    if parts.next().is_some()
        || stem.is_empty()
        || stem.len() > 8
        || extension.is_some_and(|extension| extension.is_empty() || extension.len() > 3)
        || !stem.bytes().all(valid_name_byte)
        || extension.is_some_and(|extension| !extension.bytes().all(valid_name_byte))
    {
        return Err("Name must use 8.3 format".to_owned());
    }
    Ok(())
}

const fn valid_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
fn documents_root() -> Result<PathBuf, String> {
    dirs::document_dir()
        .map(|documents| documents.join("Fanticon"))
        .ok_or_else(|| "User documents directory not found".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn canonical_directory(root: &Path, components: &[String]) -> Result<PathBuf, String> {
    let path = canonical_path(root, components)?;
    if !path.is_dir() {
        return Err("Not a directory".to_owned());
    }
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn canonical_path(root: &Path, components: &[String]) -> Result<PathBuf, String> {
    let mut path = root.to_owned();
    for component in components {
        path = case_insensitive_child(&path, component)?
            .ok_or_else(|| "Directory not found".to_owned())?;
    }
    let path = path.canonicalize().map_err(|error| io_error("path", error))?;
    if !path.starts_with(root) {
        return Err("Cannot leave root".to_owned());
    }
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn case_insensitive_child(directory: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    for entry in std::fs::read_dir(directory).map_err(|error| io_error("path", error))? {
        let entry = entry.map_err(|error| io_error("path", error))?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

#[cfg(not(target_arch = "wasm32"))]
fn relative_components(root: &Path, path: &Path) -> Result<Vec<String>, String> {
    path.strip_prefix(root)
        .map_err(|_| "Cannot leave root".to_owned())?
        .iter()
        .map(|part| {
            part.to_str()
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| "PATH IS NOT VALID UTF-8".to_owned())
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn io_error(operation: &str, error: std::io::Error) -> String {
    format!("{operation}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_filesystem_cannot_escape_root() {
        let mut filesystem = ConsoleFilesystem::memory();
        assert_eq!(filesystem.change_directory(".."), Err("Cannot leave root".to_owned()));
        assert_eq!(filesystem.remove_directory("/"), Err("Root is protected".to_owned()));
    }

    #[test]
    fn memory_directories_support_create_list_change_and_remove() {
        let mut filesystem = ConsoleFilesystem::memory();
        filesystem.create_directory("MyGame").unwrap();
        assert_eq!(filesystem.list(None).unwrap()[0].name, "mygame");
        filesystem.change_directory("MYGAME").unwrap();
        assert_eq!(filesystem.current_directory(), "/mygame");
        filesystem.change_directory("/").unwrap();
        filesystem.remove_directory("myGAME").unwrap();
        assert!(filesystem.list(None).unwrap().is_empty());
    }

    #[test]
    fn spaces_are_rejected_in_every_path_component() {
        let mut filesystem = ConsoleFilesystem::memory();
        assert_eq!(
            filesystem.create_directory("my game"),
            Err("Spaces not allowed in names".to_owned())
        );
        assert_eq!(
            filesystem.change_directory("folder name/child"),
            Err("Spaces not allowed in names".to_owned())
        );
        assert!(filesystem.list(None).unwrap().is_empty());
    }

    #[test]
    fn names_use_eight_dot_three_format() {
        let mut filesystem = ConsoleFilesystem::memory();
        filesystem.create_directory("12345678.abc").unwrap();
        assert_eq!(
            filesystem.create_directory("123456789"),
            Err("Name must use 8.3 format".to_owned())
        );
        assert_eq!(
            filesystem.create_directory("file.long"),
            Err("Name must use 8.3 format".to_owned())
        );
        assert_eq!(
            filesystem.create_directory("two.dots.txt"),
            Err("Name must use 8.3 format".to_owned())
        );
        assert_eq!(
            filesystem.create_directory("bad$name"),
            Err("Name must use 8.3 format".to_owned())
        );
    }

    #[test]
    fn memory_text_files_can_be_saved_loaded_and_listed() {
        let mut filesystem = ConsoleFilesystem::memory();
        filesystem.write_text("notes.txt", "hello\nworld").unwrap();
        assert_eq!(filesystem.read_text("NOTES.TXT").unwrap(), "hello\nworld");
        assert_eq!(
            filesystem.list(None).unwrap(),
            vec![DirectoryEntry { name: "notes.txt".to_owned(), is_directory: false }]
        );
    }

    #[test]
    fn files_can_be_removed_case_insensitively_without_removing_directories() {
        let mut filesystem = ConsoleFilesystem::memory();
        filesystem.write_text("Notes.Txt", "temporary").unwrap();
        filesystem.remove_file("NOTES.TXT").unwrap();
        assert_eq!(filesystem.read_text("notes.txt"), Err("File not found".to_owned()));

        filesystem.create_directory("assets").unwrap();
        assert_eq!(filesystem.remove_file("ASSETS"), Err("Not a file".to_owned()));
        assert_eq!(filesystem.remove_file("missing.bin"), Err("File not found".to_owned()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_filesystem_is_confined_to_its_root() {
        // The test harness names worker threads after the fully-qualified
        // test path (e.g. "host::filesystem::tests::..."). `:` is reserved
        // in Windows filenames (drive letters / alternate data streams), so
        // it has to be stripped before the name can be used as a path
        // component.
        let thread_name = std::thread::current().name().unwrap_or("worker").replace(':', "_");
        let root = std::env::temp_dir().join(format!(
            "fanticon-filesystem-test-{}-{}",
            std::process::id(),
            thread_name
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("MixCase")).unwrap();
        let mut filesystem = ConsoleFilesystem::native_for_test(&root).unwrap();
        filesystem.create_directory("project").unwrap();
        filesystem.change_directory("project").unwrap();
        assert_eq!(filesystem.current_directory(), "/project");
        assert_eq!(filesystem.change_directory("../.."), Err("Cannot leave root".to_owned()));
        filesystem.change_directory("/").unwrap();
        filesystem.remove_directory("project").unwrap();
        filesystem.change_directory("MIXCASE").unwrap();
        assert_eq!(filesystem.current_directory(), "/mixcase");
        filesystem.change_directory("/").unwrap();
        filesystem.create_directory("NewFoldr").unwrap();
        assert!(root.join("newfoldr").is_dir());
        filesystem.write_text("Notes.Txt", "native text").unwrap();
        assert_eq!(filesystem.read_text("NOTES.TXT").unwrap(), "native text");
        filesystem.remove_directory("mixcase").unwrap();
        filesystem.remove_directory("NEWFOLDR").unwrap();
        std::fs::remove_file(root.join("notes.txt")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
