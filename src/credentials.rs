use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

pub fn protect(key: &str, endpoint: &str) -> Result<String> {
    if key.is_empty() {
        return Ok(String::new());
    }
    Ok(STANDARD.encode(crypt(key.as_bytes(), endpoint, true)?))
}

pub fn unprotect(encoded: &str, endpoint: &str) -> Result<String> {
    if encoded.is_empty() {
        return Ok(String::new());
    }
    let encrypted = STANDARD.decode(encoded).context("已保存的密钥格式无效")?;
    String::from_utf8(crypt(&encrypted, endpoint, false)?).context("密钥解密结果无效")
}

fn crypt(bytes: &[u8], endpoint: &str, encrypt: bool) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().context("密钥过长")?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    // Bind the secret to its destination so editing the URL cannot reuse another service's key.
    let entropy_bytes = format!("VibeVoiceLite.AI.Key.v1\n{endpoint}");
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len().try_into().context("接口地址过长")?,
        pbData: entropy_bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI uses the current Windows user. The output allocation belongs to LocalFree.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Windows 密钥加解密失败，请重新填写 API Key");
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        for offset in 0..output.cbData as usize {
            output.pbData.add(offset).write_volatile(0);
        }
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_key_roundtrips_and_is_bound_to_endpoint() {
        let endpoint = "https://api.deepseek.com/chat/completions";
        let encrypted = protect("test-only-secret", endpoint).unwrap();
        assert!(!encrypted.contains("test-only-secret"));
        assert_eq!(unprotect(&encrypted, endpoint).unwrap(), "test-only-secret");
        assert!(unprotect(&encrypted, "https://example.com/chat/completions").is_err());
        assert!(unprotect("broken", endpoint).is_err());
        assert_eq!(protect("", endpoint).unwrap(), "");
        assert_eq!(unprotect("", endpoint).unwrap(), "");
    }
}
