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
enum StoreBackend {
    File(PathBuf),
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    Keychain(String),
    Ephemeral(std::sync::Arc<std::sync::Mutex<Vec<PairingRecord>>>),
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    backend: StoreBackend,
    fallback_path: PathBuf,
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
    #[error("keychain operation failed: {0}")]
    Keychain(String),
}

impl PairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let p = path.into();
        Self {
            fallback_path: p.clone(),
            backend: StoreBackend::File(p),
        }
    }

    pub fn new_ephemeral() -> Self {
        Self {
            backend: StoreBackend::Ephemeral(std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))),
            fallback_path: PathBuf::new(),
        }
    }

    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub fn new_keychain(service: impl Into<String>) -> Self {
        Self {
            backend: StoreBackend::Keychain(service.into()),
            fallback_path: PathBuf::new(),
        }
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
        #[cfg(target_os = "ios")]
        {
            Ok(Self::new_keychain("com.eclipticrd.ios.pairing"))
        }
        #[cfg(not(target_os = "ios"))]
        {
            Ok(Self::new(Self::default_path()?))
        }
    }

    pub fn path(&self) -> &Path {
        match &self.backend {
            StoreBackend::File(p) => p.as_path(),
            _ => &self.fallback_path,
        }
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, PairingStoreError> {
        match &self.backend {
            StoreBackend::File(path) => match fs::read(path) {
                Ok(bytes) => {
                    let records: Vec<PairingRecord> = serde_json::from_slice(&bytes)?;
                    for record in &records {
                        record.key_array()?;
                    }
                    Ok(records)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(error) => Err(error.into()),
            },
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => load_all_keychain(service),
            StoreBackend::Ephemeral(records) => {
                let guard = records
                    .lock()
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "ephemeral store poisoned"))?;
                Ok(guard.clone())
            }
        }
    }

    pub fn load(&self, id: &str) -> Result<Option<PairingRecord>, PairingStoreError> {
        match &self.backend {
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => load_keychain(service, id),
            _ => Ok(self.load_all()?.into_iter().find(|record| record.id == id)),
        }
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
            .rfind(|record| record.name.eq_ignore_ascii_case(host_name) || record.id == host_name))
    }

    pub fn save(&self, record: PairingRecord) -> Result<(), PairingStoreError> {
        record.key_array()?;
        match &self.backend {
            StoreBackend::File(path) => {
                let mut records = self.load_all()?;
                records.retain(|existing| existing.id != record.id);
                records.push(record);
                self.write_records(path, &records)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => save_keychain(service, &record),
            StoreBackend::Ephemeral(records) => {
                let mut guard = records
                    .lock()
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "ephemeral store poisoned"))?;
                guard.retain(|existing| existing.id != record.id);
                guard.push(record);
                Ok(())
            }
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), PairingStoreError> {
        match &self.backend {
            StoreBackend::File(path) => {
                let mut records = self.load_all()?;
                records.retain(|record| record.id != id);
                self.write_records(path, &records)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => delete_keychain(service, id),
            StoreBackend::Ephemeral(records) => {
                let mut guard = records
                    .lock()
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "ephemeral store poisoned"))?;
                guard.retain(|record| record.id != id);
                Ok(())
            }
        }
    }

    fn write_records(&self, path: &Path, records: &[PairingRecord]) -> Result<(), PairingStoreError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "pairing store path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(records)?;
        write_private(&temporary, &bytes)?;
        fs::rename(&temporary, path)?;
        set_private_permissions(path)?;
        Ok(())
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[allow(dead_code)]
mod security_ffi {
    use std::os::raw::c_void;

    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFDataRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFArrayRef = *const c_void;
    pub type CFIndex = isize;
    pub type OSStatus = i32;

    pub const ERR_SEC_SUCCESS: OSStatus = 0;
    pub const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
    pub const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        pub static kSecClass: CFStringRef;
        pub static kSecClassGenericPassword: CFTypeRef;
        pub static kSecAttrService: CFStringRef;
        pub static kSecAttrAccount: CFStringRef;
        pub static kSecValueData: CFStringRef;
        pub static kSecReturnData: CFStringRef;
        pub static kSecReturnAttributes: CFStringRef;
        pub static kSecMatchLimit: CFStringRef;
        pub static kSecMatchLimitOne: CFTypeRef;
        pub static kSecMatchLimitAll: CFTypeRef;
        pub static kSecAttrAccessible: CFStringRef;
        pub static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: CFTypeRef;

        pub fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemUpdate(query: CFDictionaryRef, attributesToUpdate: CFDictionaryRef) -> OSStatus;
        pub fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFBooleanTrue: CFTypeRef;
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        pub fn CFStringCreateWithBytes(
            alloc: CFTypeRef,
            bytes: *const u8,
            numBytes: CFIndex,
            encoding: u32,
            isExternalRepresentation: u8,
        ) -> CFStringRef;
        pub fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        pub fn CFDataGetLength(theData: CFDataRef) -> CFIndex;
        pub fn CFDataGetBytePtr(theData: CFDataRef) -> *const u8;
        pub fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            numValues: CFIndex,
            keyCallBacks: *const c_void,
            valueCallBacks: *const c_void,
        ) -> CFDictionaryRef;
        pub fn CFDictionaryGetValue(theDict: CFDictionaryRef, theKey: CFTypeRef) -> CFTypeRef;
        pub fn CFArrayGetCount(theArray: CFArrayRef) -> CFIndex;
        pub fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        pub fn CFStringGetCString(
            theString: CFStringRef,
            buffer: *mut u8,
            bufferSize: CFIndex,
            encoding: u32,
        ) -> u8;
        pub fn CFStringGetLength(theString: CFStringRef) -> CFIndex;
        pub fn CFRelease(cf: CFTypeRef);
    }
    pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
struct CfWrapper<T>(*const T);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl<T> Drop for CfWrapper<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                security_ffi::CFRelease(self.0 as security_ffi::CFTypeRef);
            }
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_string(s: &str) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            security_ffi::K_CF_STRING_ENCODING_UTF8,
            0,
        );
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_data(bytes: &[u8]) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
        );
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_dictionary(
    pairs: &[(security_ffi::CFTypeRef, security_ffi::CFTypeRef)],
) -> Option<CfWrapper<std::os::raw::c_void>> {
    let mut keys = Vec::with_capacity(pairs.len());
    let mut values = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        keys.push(*k);
        values.push(*v);
    }
    unsafe {
        let dict = security_ffi::CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as isize,
            &security_ffi::kCFTypeDictionaryKeyCallBacks as *const _ as *const _,
            &security_ffi::kCFTypeDictionaryValueCallBacks as *const _ as *const _,
        );
        if dict.is_null() {
            None
        } else {
            Some(CfWrapper(dict))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn cf_data_to_vec(data: security_ffi::CFDataRef) -> Vec<u8> {
    unsafe {
        let len = security_ffi::CFDataGetLength(data) as usize;
        let ptr = security_ffi::CFDataGetBytePtr(data);
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(ptr, len).to_vec()
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn save_keychain(service: &str, record: &PairingRecord) -> Result<(), PairingStoreError> {
    let key = format!("erd_pairing_{}", record.id);
    let bytes = serde_json::to_vec(record)?;
    unsafe {
        let service_cf = make_cf_string(service)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate service CFString".into()))?;
        let account_cf = make_cf_string(&key)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate account CFString".into()))?;
        let data_cf = make_cf_data(&bytes)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate data CFData".into()))?;

        let pairs = [
            (security_ffi::kSecClass as security_ffi::CFTypeRef, security_ffi::kSecClassGenericPassword),
            (security_ffi::kSecAttrService as security_ffi::CFTypeRef, service_cf.0),
            (security_ffi::kSecAttrAccount as security_ffi::CFTypeRef, account_cf.0),
            (security_ffi::kSecValueData as security_ffi::CFTypeRef, data_cf.0),
            (security_ffi::kSecAttrAccessible as security_ffi::CFTypeRef, security_ffi::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly),
        ];
        let dict = make_cf_dictionary(&pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate CFDictionary".into()))?;

        let status = security_ffi::SecItemAdd(dict.0, std::ptr::null_mut());
        if status == security_ffi::ERR_SEC_DUPLICATE_ITEM {
            let query_pairs = [
                (security_ffi::kSecClass as security_ffi::CFTypeRef, security_ffi::kSecClassGenericPassword),
                (security_ffi::kSecAttrService as security_ffi::CFTypeRef, service_cf.0),
                (security_ffi::kSecAttrAccount as security_ffi::CFTypeRef, account_cf.0),
            ];
            let query_dict = make_cf_dictionary(&query_pairs)
                .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate query CFDictionary".into()))?;
            let update_pairs = [
                (security_ffi::kSecValueData as security_ffi::CFTypeRef, data_cf.0),
            ];
            let update_dict = make_cf_dictionary(&update_pairs)
                .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate update CFDictionary".into()))?;

            let update_status = security_ffi::SecItemUpdate(query_dict.0, update_dict.0);
            if update_status != security_ffi::ERR_SEC_SUCCESS {
                return Err(PairingStoreError::Keychain(format!("SecItemUpdate failed: OSStatus {update_status}")));
            }
            Ok(())
        } else if status != security_ffi::ERR_SEC_SUCCESS {
            Err(PairingStoreError::Keychain(format!("SecItemAdd failed: OSStatus {status}")))
        } else {
            Ok(())
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn load_keychain(service: &str, id: &str) -> Result<Option<PairingRecord>, PairingStoreError> {
    let key = format!("erd_pairing_{id}");
    unsafe {
        let service_cf = make_cf_string(service)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate service CFString".into()))?;
        let account_cf = make_cf_string(&key)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate account CFString".into()))?;

        let query_pairs = [
            (security_ffi::kSecClass as security_ffi::CFTypeRef, security_ffi::kSecClassGenericPassword),
            (security_ffi::kSecAttrService as security_ffi::CFTypeRef, service_cf.0),
            (security_ffi::kSecAttrAccount as security_ffi::CFTypeRef, account_cf.0),
            (security_ffi::kSecReturnData as security_ffi::CFTypeRef, security_ffi::kCFBooleanTrue),
            (security_ffi::kSecMatchLimit as security_ffi::CFTypeRef, security_ffi::kSecMatchLimitOne),
        ];
        let query = make_cf_dictionary(&query_pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate query CFDictionary".into()))?;

        let mut result: security_ffi::CFTypeRef = std::ptr::null();
        let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
        if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            return Ok(None);
        }
        if status != security_ffi::ERR_SEC_SUCCESS {
            return Err(PairingStoreError::Keychain(format!("SecItemCopyMatching failed: OSStatus {status}")));
        }
        if result.is_null() {
            return Ok(None);
        }
        let wrapper = CfWrapper(result);
        let bytes = cf_data_to_vec(wrapper.0);
        let record: PairingRecord = serde_json::from_slice(&bytes)?;
        record.key_array()?;
        Ok(Some(record))
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn load_all_keychain(service: &str) -> Result<Vec<PairingRecord>, PairingStoreError> {
    unsafe {
        let service_cf = make_cf_string(service)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate service CFString".into()))?;

        let query_pairs = [
            (security_ffi::kSecClass as security_ffi::CFTypeRef, security_ffi::kSecClassGenericPassword),
            (security_ffi::kSecAttrService as security_ffi::CFTypeRef, service_cf.0),
            (security_ffi::kSecReturnAttributes as security_ffi::CFTypeRef, security_ffi::kCFBooleanTrue),
            (security_ffi::kSecReturnData as security_ffi::CFTypeRef, security_ffi::kCFBooleanTrue),
            (security_ffi::kSecMatchLimit as security_ffi::CFTypeRef, security_ffi::kSecMatchLimitAll),
        ];
        let query = make_cf_dictionary(&query_pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate query CFDictionary".into()))?;

        let mut result: security_ffi::CFTypeRef = std::ptr::null();
        let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
        if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            return Ok(Vec::new());
        }
        if status != security_ffi::ERR_SEC_SUCCESS {
            return Err(PairingStoreError::Keychain(format!("SecItemCopyMatching list failed: OSStatus {status}")));
        }
        if result.is_null() {
            return Ok(Vec::new());
        }
        let wrapper = CfWrapper(result);
        let count = security_ffi::CFArrayGetCount(wrapper.0);
        let mut records = Vec::new();
        for i in 0..count {
            let dict = security_ffi::CFArrayGetValueAtIndex(wrapper.0, i);
            if !dict.is_null() {
                let data_val = security_ffi::CFDictionaryGetValue(dict, security_ffi::kSecValueData as security_ffi::CFTypeRef);
                if !data_val.is_null() {
                    let bytes = cf_data_to_vec(data_val);
                    if let Ok(record) = serde_json::from_slice::<PairingRecord>(&bytes) {
                        if record.key_array().is_ok() {
                            records.push(record);
                        }
                    }
                }
            }
        }
        Ok(records)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn delete_keychain(service: &str, id: &str) -> Result<(), PairingStoreError> {
    let key = format!("erd_pairing_{id}");
    unsafe {
        let service_cf = make_cf_string(service)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate service CFString".into()))?;
        let account_cf = make_cf_string(&key)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate account CFString".into()))?;

        let query_pairs = [
            (security_ffi::kSecClass as security_ffi::CFTypeRef, security_ffi::kSecClassGenericPassword),
            (security_ffi::kSecAttrService as security_ffi::CFTypeRef, service_cf.0),
            (security_ffi::kSecAttrAccount as security_ffi::CFTypeRef, account_cf.0),
        ];
        let query = make_cf_dictionary(&query_pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate query CFDictionary".into()))?;

        let status = security_ffi::SecItemDelete(query.0);
        if status == security_ffi::ERR_SEC_SUCCESS || status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            Ok(())
        } else {
            Err(PairingStoreError::Keychain(format!("SecItemDelete failed: OSStatus {status}")))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ephemeral_pairing_store_roundtrip_and_delete() {
        let store = PairingStore::new_ephemeral();
        let record = PairingRecord {
            id: "host-ephemeral-1".into(),
            name: "Server".into(),
            key: vec![0x55; 32],
            added_at_unix_ms: 1700000000000,
        };

        store.save(record.clone()).unwrap();
        assert_eq!(store.load("host-ephemeral-1").unwrap(), Some(record.clone()));
        assert_eq!(store.find_by_host("server").unwrap(), Some(record.clone()));
        assert_eq!(store.find_by_host("host-ephemeral-1").unwrap(), Some(record.clone()));

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "host-ephemeral-1");

        store.delete("host-ephemeral-1").unwrap();
        assert_eq!(store.load("host-ephemeral-1").unwrap(), None);
        assert!(store.load_all().unwrap().is_empty());
    }
}
