//! Optional Scala file catalogue. Called only by the editor, never process().
use std::{io::ErrorKind, path::Path};

#[derive(Default)]
pub(super) struct ScaleLibrary {
    pub scales: Vec<String>,
    pub mappings: Vec<String>,
    pub status: String,
}

impl ScaleLibrary {
    pub fn discover() -> Self {
        match std::env::var_os("HOME").filter(|home| !home.is_empty()) {
            Some(home) => Self::scan(&Path::new(&home).join(".scales")),
            None => Self {
                status: "HOME не задан: укажите путь к Scala вручную.".into(),
                ..Self::default()
            },
        }
    }

    fn scan(directory: &Path) -> Self {
        let mut library = Self::default();
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                library.status = if error.kind() == ErrorKind::NotFound {
                    "Папка ~/.scales/ не найдена. Можно указать пути вручную.".into()
                } else {
                    format!("Не удалось прочитать {}: {error}", directory.display())
                };
                return library;
            }
        };
        let mut skipped = 0;
        for entry in entries {
            let Ok(entry) = entry else {
                skipped += 1;
                continue;
            };
            let path = entry.path();
            let Some(extension) = path.extension().and_then(|s| s.to_str()) else {
                continue;
            };
            let destination = if extension.eq_ignore_ascii_case("scl") {
                &mut library.scales
            } else if extension.eq_ignore_ascii_case("kbm") {
                &mut library.mappings
            } else {
                continue;
            };
            // Follow file symlinks, but do not recurse into directories.
            if !path.is_file() {
                continue;
            }
            if let Some(path) = path.to_str() {
                destination.push(path.to_owned());
            } else {
                // Text path controls cannot represent non-UTF-8 filenames.
                skipped += 1;
            }
        }
        library.scales.sort();
        library.mappings.sort();
        library.status = format!(
            "Найдено: {} .scl, {} .kbm",
            library.scales.len(),
            library.mappings.len()
        );
        if skipped != 0 {
            library.status.push_str(&format!(
                ". Пропущено недоступных записей или имён не в UTF-8: {skipped}"
            ));
        }
        library
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, time::SystemTime};

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "adaptive-ji-scale-library-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn optional_folder_can_appear_and_disappear_without_breaking_catalogue() {
        let temp = TestDirectory::new();
        let folder = temp.0.join(".scales");
        let missing = ScaleLibrary::scan(&folder);
        assert!(missing.scales.is_empty() && missing.mappings.is_empty());
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("13 equal divisions.scl"), "scale contents").unwrap();
        fs::write(folder.join("13 equal divisions.kbm"), "mapping contents").unwrap();
        let present = ScaleLibrary::scan(&folder);
        assert_eq!(present.scales.len(), 1);
        assert_eq!(present.mappings.len(), 1);
        fs::remove_dir_all(&folder).unwrap();
        let missing = ScaleLibrary::scan(&folder);
        assert!(missing.scales.is_empty() && missing.mappings.is_empty());
        fs::write(&folder, "not a directory").unwrap();
        let invalid = ScaleLibrary::scan(&folder);
        assert!(invalid.scales.is_empty() && invalid.mappings.is_empty());
        assert!(invalid.status.contains("Не удалось"));
    }

    #[test]
    fn catalogue_filters_and_sorts_files_without_parsing_or_recursing() {
        let temp = TestDirectory::new();
        for name in ["я.scl", "a.SCL", "z.scl", "map.KBM", "ignored.txt"] {
            fs::write(temp.0.join(name), "not parsed during discovery").unwrap();
        }
        fs::create_dir(temp.0.join("directory.scl")).unwrap();
        fs::write(temp.0.join("directory.scl/nested.scl"), "ignored").unwrap();
        let library = ScaleLibrary::scan(&temp.0);
        let names: Vec<_> = library
            .scales
            .iter()
            .map(|p| Path::new(p).file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["a.SCL", "z.scl", "я.scl"]);
        assert_eq!(library.mappings.len(), 1);
        assert_eq!(library.scales, ScaleLibrary::scan(&temp.0).scales);
    }
}
