use std::slice;

use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::w,
};

const ENTROPY: &[u8] = b"fjarsyn/local-identity/v1";

pub(super) fn protect(plaintext: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = blob(plaintext);
    let entropy = blob(ENTROPY);
    let mut output = OwnedBlob::default();

    // SAFETY: every input blob points to a live immutable slice for the
    // duration of the call. DPAPI initializes `output`, whose allocation is
    // retained by `OwnedBlob` and released exactly once with `LocalFree`.
    unsafe {
        CryptProtectData(
            &input,
            w!("Fjarsyn local identity"),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )?;
    }

    Ok(output.copy())
}

pub(super) fn unprotect(ciphertext: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = blob(ciphertext);
    let entropy = blob(ENTROPY);
    let mut output = OwnedBlob::default();

    // SAFETY: the input and entropy blobs borrow live slices. DPAPI owns the
    // returned allocation until `OwnedBlob` releases it with `LocalFree`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )?;
    }

    Ok(output.copy_and_zero())
}

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr().cast_mut() }
}

#[derive(Default)]
struct OwnedBlob(CRYPT_INTEGER_BLOB);

impl OwnedBlob {
    fn copy(&self) -> Vec<u8> {
        if self.0.pbData.is_null() || self.0.cbData == 0 {
            return Vec::new();
        }

        // SAFETY: DPAPI returned `cbData` initialized bytes at `pbData`, and
        // `self` retains that allocation until after the copy completes.
        unsafe { slice::from_raw_parts(self.0.pbData, self.0.cbData as usize).to_vec() }
    }

    fn copy_and_zero(&mut self) -> Vec<u8> {
        if self.0.pbData.is_null() || self.0.cbData == 0 {
            return Vec::new();
        }

        // SAFETY: DPAPI returned `cbData` initialized bytes at `pbData`. The
        // allocation remains uniquely owned here, so erase its plaintext after
        // copying and before `Drop` releases it with `LocalFree`.
        let bytes = unsafe { slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize) };
        let copied = bytes.to_vec();
        bytes.fill(0);
        copied
    }
}

impl Drop for OwnedBlob {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            // SAFETY: DPAPI allocated this buffer with LocalAlloc and ownership
            // has not left `OwnedBlob`.
            unsafe {
                LocalFree(HLOCAL(self.0.pbData.cast()));
            }
            self.0 = CRYPT_INTEGER_BLOB::default();
        }
    }
}
