use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use base64::prelude::*;
use erd_proto::PAIRING_KEY_SIZE;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

const SWIFT_REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRecord {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "deserialize_key", serialize_with = "serialize_key")]
    pub key: Vec<u8>,
    #[serde(
        rename = "addedAt",
        default,
        deserialize_with = "deserialize_added_at",
        serialize_with = "serialize_added_at",
        alias = "addedAt",
        alias = "added_at",
        alias = "added_at_unix_ms",
        alias = "addedAtUnixMs"
    )]
    pub added_at_unix_ms: u64,
}

fn deserialize_key<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct KeyVisitor;

    impl<'de> de::Visitor<'de> for KeyVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a base64 string or byte array")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            BASE64_STANDARD
                .decode(value.trim())
                .map_err(de::Error::custom)
        }

        fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            self.visit_str(value)
        }

        fn visit_string<E>(self, value: String) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            Ok(value.to_vec())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<u8>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut bytes = Vec::new();
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }

    deserializer.deserialize_any(KeyVisitor)
}

fn serialize_key<S>(key: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = BASE64_STANDARD.encode(key);
    serializer.serialize_str(&encoded)
}

fn deserialize_added_at<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    struct AddedAtVisitor;

    impl<'de> de::Visitor<'de> for AddedAtVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a timestamp number")
        }

        fn visit_i64<E>(self, value: i64) -> Result<u64, E>
        where
            E: de::Error,
        {
            Ok(value.max(0) as u64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<u64, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_f64<E>(self, value: f64) -> Result<u64, E>
        where
            E: de::Error,
        {
            if value > 1_000_000_000_000.0 {
                // Already unix ms
                Ok(value as u64)
            } else if value > 1_000_000_000.0 {
                // Unix seconds
                Ok((value * 1000.0) as u64)
            } else {
                // Swift reference date seconds (since 2001-01-01)
                let unix_secs = value + SWIFT_REFERENCE_DATE_OFFSET;
                Ok((unix_secs.max(0.0) * 1000.0) as u64)
            }
        }
    }

    deserializer.deserialize_any(AddedAtVisitor)
}

fn serialize_added_at<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let swift_time = (*value as f64 / 1000.0) - SWIFT_REFERENCE_DATE_OFFSET;
    serializer.serialize_f64(swift_time)
}

impl PairingRecord {
    pub fn key_array(&self) -> Result<[u8; PAIRING_KEY_SIZE], PairingStoreError> {
        self.key
            .as_slice()
            .try_into()
            .map_err(|_| PairingStoreError::InvalidKeyLength(self.key.len()))
    }
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum PairingStoreError {
    #[error("pairing store I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("pairing store JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("pairing key has length {0}, expected 32")]
    InvalidKeyLength(usize),
    #[error("could not determine the user application data directory")]
    NoApplicationDataDirectory,
}

impl PairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> Result<PathBuf, PairingStoreError> {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            Ok(home
                .join("Library")
                .join("Application Support")
                .join("EclipticRD")
                .join("pairing-keys.json"))
        }
        #[cfg(target_os = "windows")]
        {
            let app_data = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            return Ok(app_data.join("EclipticRD").join("pairing-keys.json"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            return Ok(base.join("EclipticRD").join("pairing-keys.json"));
        }
    }

    pub fn open_default() -> Result<Self, PairingStoreError> {
        Ok(Self::new(Self::default_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, PairingStoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let records: Vec<PairingRecord> = serde_json::from_slice(&bytes)?;
                for record in &records {
                    record.key_array()?;
                }
                Ok(records)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn load(&self, id: &str) -> Result<Option<PairingRecord>, PairingStoreError> {
        Ok(self.load_all()?.into_iter().find(|record| record.id == id))
    }

    /// Latest record whose host name matches `host_name` — the Parsec-style
    /// "connect by computer name" lookup for reconnects without a PIN.
    pub fn find_by_host(
        &self,
        host_name: &str,
    ) -> Result<Option<PairingRecord>, PairingStoreError> {
        Ok(self
            .load_all()?
            .into_iter()
            .rfind(|record| record.name.eq_ignore_ascii_case(host_name)))
    }

    pub fn save(&self, record: PairingRecord) -> Result<(), PairingStoreError> {
        record.key_array()?;
        let mut records = self.load_all()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record);
        self.write_records(&records)
    }

    pub fn delete(&self, id: &str) -> Result<(), PairingStoreError> {
        let mut records = self.load_all()?;
        records.retain(|record| record.id != id);
        self.write_records(&records)
    }

    fn write_records(&self, records: &[PairingRecord]) -> Result<(), PairingStoreError> {
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "pairing store path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(records)?;
        write_private(&temporary, &bytes)?;
        fs::rename(&temporary, &self.path)?;
        set_private_permissions(&self.path)?;
        Ok(())
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

fn set_private_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
