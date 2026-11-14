//! Lossless raw-body storage with bounded, verified recovery and legacy decoding.
use super::fault;
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use taskloop::Result;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Encoding {
    #[default]
    Identity,
    Gzip,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedBody {
    #[serde(default)]
    encoding: Encoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body_bytes: Option<usize>,
    body_sha256: String,
    base64: String,
}
impl ArchivedBody {
    pub(crate) fn capture(body: &[u8]) -> Result<Self> {
        if body.len() > crate::net::BODY_LIMIT {
            return Err(fault("原文超过单页字节边界"));
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(body)?;
        Ok(Self {
            encoding: Encoding::Gzip,
            body_bytes: Some(body.len()),
            body_sha256: crate::domain::hash(body),
            base64: STANDARD.encode(gzip.finish()?),
        })
    }
    /// Restore exact acquired bytes, including legacy identity archives, within the fetch limit.
    pub fn restore(&self) -> Result<Vec<u8>> {
        let limit = crate::net::BODY_LIMIT;
        if self.base64.len() > super::ARCHIVE_BYTES {
            return Err(fault("原文编码超过档案字节边界"));
        }
        if self.body_bytes.is_some_and(|n| n > limit) {
            return Err(fault("原文长度超过单页字节边界"));
        }
        let encoded = STANDARD
            .decode(&self.base64)
            .map_err(|_| fault("原文编码无效"))?;
        let bytes = match self.encoding {
            Encoding::Identity => encoded,
            Encoding::Gzip => {
                if self.body_bytes.is_none() {
                    return Err(fault("压缩原文缺少长度声明"));
                }
                let mut bytes = Vec::new();
                GzDecoder::new(encoded.as_slice())
                    .take(limit as u64 + 1)
                    .read_to_end(&mut bytes)?;
                bytes
            }
        };
        if bytes.len() > limit
            || self.body_bytes.is_some_and(|n| n != bytes.len())
            || crate::domain::hash(&bytes) != self.body_sha256
        {
            return Err(fault("原文长度或摘要不匹配"));
        }
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compressed_and_legacy_archives_restore_identical_source_bytes() {
        let body = b"<html>the exact original source</html>";
        let packed = ArchivedBody::capture(body).unwrap();
        assert_eq!(packed.restore().unwrap(), body);
        let legacy: ArchivedBody = serde_json::from_value(
            json!({"body_sha256":crate::domain::hash(body),"base64":STANDARD.encode(body)}),
        )
        .unwrap();
        assert_eq!(legacy.restore().unwrap(), body);
    }
    #[test]
    fn corrupt_unknown_or_expanding_archives_never_become_verified_bodies() {
        let mut packed = ArchivedBody::capture(b"source").unwrap();
        packed.body_bytes = Some(2);
        assert!(packed.restore().is_err());
        packed.body_bytes = Some(6);
        let mut bytes = STANDARD.decode(&packed.base64).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        packed.base64 = STANDARD.encode(bytes);
        assert!(packed.restore().is_err());
        assert!(
            serde_json::from_value::<ArchivedBody>(
                json!({"encoding":"unknown","base64":"","body_sha256":""})
            )
            .is_err()
        );
        let encoded_overflow: ArchivedBody = serde_json::from_value(json!({
            "base64": "A".repeat(super::super::ARCHIVE_BYTES + 1),
            "body_sha256": ""
        }))
        .unwrap();
        assert!(encoded_overflow.restore().is_err());
        let big = vec![b'a'; crate::net::BODY_LIMIT + 1];
        assert!(ArchivedBody::capture(&big).is_err());
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(&big).unwrap();
        let oversized:ArchivedBody=serde_json::from_value(json!({"encoding":"gzip","body_bytes":1,"base64":STANDARD.encode(gzip.finish().unwrap()),"body_sha256":crate::domain::hash(&big)})).unwrap();
        assert!(oversized.restore().is_err());
    }
}
