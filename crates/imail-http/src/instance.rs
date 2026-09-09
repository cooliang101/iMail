//! Persistent identity of a service data directory.
use crate::HttpAdapterError;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};
use uuid::{Uuid, Version};

pub(super) fn load_or_create_instance_id(data_dir: &Path) -> Result<String, HttpAdapterError> {
    let file = data_dir.join("instance-id");
    match fs::read_to_string(&file) {
        Ok(value) => validate_instance_id(value.trim()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(data_dir)?;
            let created = Uuid::new_v4().to_string();
            match OpenOptions::new().write(true).create_new(true).open(&file) {
                Ok(mut output) => {
                    output.write_all(created.as_bytes())?;
                    output.write_all(b"\n")?;
                    output.sync_all()?;
                    Ok(created)
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    validate_instance_id(fs::read_to_string(file)?.trim())
                }
                Err(error) => Err(error.into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_instance_id(value: &str) -> Result<String, HttpAdapterError> {
    let parsed = Uuid::parse_str(value).map_err(|_| HttpAdapterError::InvalidInstanceId)?;
    if parsed.get_version() != Some(Version::Random) {
        return Err(HttpAdapterError::InvalidInstanceId);
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("imail-instance-test-{}", Uuid::new_v4())))
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_a_persistent_v4_identity_and_reuses_it_after_reopening() {
        let directory = Directory::new();
        let first = load_or_create_instance_id(&directory.0).unwrap();
        assert_eq!(
            Uuid::parse_str(&first).unwrap().get_version(),
            Some(Version::Random)
        );
        let persisted = fs::read(directory.0.join("instance-id")).unwrap();
        assert_eq!(persisted, format!("{first}\n").as_bytes());
        assert_eq!(load_or_create_instance_id(&directory.0).unwrap(), first);
        assert_eq!(
            fs::read(directory.0.join("instance-id")).unwrap(),
            persisted
        );
    }

    #[test]
    fn refuses_corrupt_or_non_v4_identities_without_overwriting_them() {
        let directory = Directory::new();
        fs::create_dir_all(&directory.0).unwrap();
        for value in [
            "",
            "corrupt",
            "00000000-0000-0000-0000-000000000000",
            "550e8400-e29b-11d4-a716-446655440000",
        ] {
            fs::write(directory.0.join("instance-id"), value).unwrap();
            assert!(matches!(
                load_or_create_instance_id(&directory.0),
                Err(HttpAdapterError::InvalidInstanceId)
            ));
            assert_eq!(
                fs::read_to_string(directory.0.join("instance-id")).unwrap(),
                value
            );
        }
    }

    #[test]
    fn normalizes_existing_identity_without_rewriting_the_file() {
        let directory = Directory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let value = "  550E8400-E29B-41D4-A716-446655440000\r\n";
        fs::write(directory.0.join("instance-id"), value).unwrap();
        assert_eq!(
            load_or_create_instance_id(&directory.0).unwrap(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            fs::read_to_string(directory.0.join("instance-id")).unwrap(),
            value
        );
    }
}
