//! OS credential storage only. Cloud owns references, validation and transactions.
#[cfg(target_os = "macos")]
mod backend {
    use security_framework::{os::macos::keychain::SecKeychain, passwords::*};
    pub fn read(service: &str, reference: &str) -> Result<Option<String>, String> {
        let _no_ui = SecKeychain::disable_user_interaction()
            .map_err(|e| format!("keychain_read_{}", e.code()))?;
        match generic_password(PasswordOptions::new_generic_password(service, reference)) {
            Ok(secret) => String::from_utf8(secret)
                .map(Some)
                .map_err(|_| "cloud credential is not UTF-8".into()),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(e) => Err(format!("keychain_read_{}", e.code())),
        }
    }
    pub fn save(service: &str, reference: &str, secret: &[u8]) -> Result<(), String> {
        set_generic_password(service, reference, secret)
            .map_err(|e| format!("keychain_save_{}", e.code()))
    }
    pub fn delete(service: &str, reference: &str) -> Result<(), String> {
        delete_generic_password(service, reference)
            .map_err(|e| format!("keychain_delete_{}", e.code()))
    }
}
#[cfg(windows)]
mod backend {
    use std::{mem, ptr};
    use windows_sys::Win32::{Foundation::*, Security::Credentials::*};
    fn target(service: &str, reference: &str) -> Vec<u16> {
        loppi_protocol::platform::wide(format!("{service}:{reference}"))
    }
    pub fn read(service: &str, reference: &str) -> Result<Option<String>, String> {
        unsafe {
            let mut credential = ptr::null_mut();
            let name = target(service, reference);
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                let error = GetLastError();
                return if error == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(format!("credential_read_{error}"))
                };
            }
            let bytes = if (*credential).CredentialBlobSize == 0 {
                vec![]
            } else {
                std::slice::from_raw_parts(
                    (*credential).CredentialBlob,
                    (*credential).CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(credential.cast());
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "cloud credential is not UTF-8".into())
        }
    }
    pub fn save(service: &str, reference: &str, secret: &[u8]) -> Result<(), String> {
        if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err("credential_size_limit".into());
        }
        let name = target(service, reference);
        let mut value: CREDENTIALW = unsafe { mem::zeroed() };
        value.Type = CRED_TYPE_GENERIC;
        value.TargetName = name.as_ptr() as _;
        value.CredentialBlobSize = secret.len() as u32;
        value.CredentialBlob = secret.as_ptr() as _;
        value.Persist = CRED_PERSIST_LOCAL_MACHINE;
        if unsafe { CredWriteW(&value, 0) } == 0 {
            return Err(format!("credential_save_{}", unsafe { GetLastError() }));
        }
        Ok(())
    }
    pub fn delete(service: &str, reference: &str) -> Result<(), String> {
        let name = target(service, reference);
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_NOT_FOUND {
                return Err(format!("credential_delete_{error}"));
            }
        }
        Ok(())
    }
}
pub use backend::*;

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn synthetic_credential_roundtrip_is_scoped_and_removed() {
        let service = format!("app.loppi.test.{}", uuid::Uuid::new_v4());
        let reference = "synthetic-credential";
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = super::delete(&self.0, "synthetic-credential");
            }
        }
        let _cleanup = Cleanup(service.clone());
        assert!(super::read(&service, reference).unwrap().is_none());
        super::save(&service, reference, b"synthetic-local-test-value").unwrap();
        assert_eq!(
            super::read(&service, reference).unwrap().as_deref(),
            Some("synthetic-local-test-value")
        );
        super::delete(&service, reference).unwrap();
        assert!(super::read(&service, reference).unwrap().is_none());
    }
}
