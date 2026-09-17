use crate::error::{AppError, AppResult};
use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Decrypt AES-128-CBC HLS segment. IV is 16 bytes; if missing, use media sequence as big-endian IV.
pub fn decrypt_aes128(data: &[u8], key: &[u8], iv: &[u8; 16]) -> AppResult<Vec<u8>> {
    if key.len() != 16 {
        return Err(AppError::msg(format!(
            "AES-128 key must be 16 bytes, got {}",
            key.len()
        )));
    }
    let decryptor = Aes128CbcDec::new_from_slices(key, iv)
        .map_err(|e| AppError::msg(format!("Invalid AES key/iv: {e}")))?;
    let mut buf = data.to_vec();
    let decrypted = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| AppError::msg(format!("AES decrypt failed: {e}")))?;
    Ok(decrypted.to_vec())
}

pub fn parse_iv(iv_hex: Option<&str>, media_sequence: u64, segment_index: u64) -> AppResult<[u8; 16]> {
    if let Some(raw) = iv_hex {
        let hex_str = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
        let bytes = hex::decode(hex_str)
            .map_err(|e| AppError::msg(format!("Invalid IV hex: {e}")))?;
        if bytes.len() != 16 {
            return Err(AppError::msg(format!(
                "IV must be 16 bytes, got {}",
                bytes.len()
            )));
        }
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&bytes);
        return Ok(iv);
    }
    // HLS default: IV is the media sequence number of the segment as 128-bit big-endian
    let seq = media_sequence.saturating_add(segment_index);
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&seq.to_be_bytes());
    Ok(iv)
}
