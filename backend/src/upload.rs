//! multipart 表单的读取：文本字段 + 单个文件。
//!
//! 物种照片和鸟调 GPX 都用它：字段名是 `file` 的那个按文件读进内存，
//! 其余按 UTF-8 文本字段。文件大小在累积过程中就卡住，不会等整个请求读完
//! 才发现超限。

use crate::errors::ServiceError;
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use std::collections::HashMap;

/// 单个文本字段的上限。拍摄人 / 地点 / 领队 / 天气都很短，卡住它是为了不让一个
/// 超大表单把内存打满。
pub const MAX_TEXT_FIELD_BYTES: usize = 64 * 1024;

/// 一个已读进内存的上传文件。
pub struct UploadedFile {
    pub filename: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// 把 multipart 拆成「文本字段表 + 文件」。
///
/// `max_file_bytes` 是单个文件的上限，`file_label` 只用在报错文案里
/// （例如 "photo" / "GPX"）。
pub async fn read_multipart(
    mut payload: Multipart,
    max_file_bytes: usize,
    file_label: &str,
) -> Result<(HashMap<String, String>, Option<UploadedFile>), ServiceError> {
    let mut fields = HashMap::new();
    let mut file: Option<UploadedFile> = None;

    while let Some(mut field) = payload.try_next().await.map_err(multipart_error)? {
        let name = match field.content_disposition().and_then(|cd| cd.get_name()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if name == "file" {
            if file.is_some() {
                return Err(ServiceError::ValidationError(format!(
                    "only one {file_label} file is allowed"
                )));
            }
            let filename = field
                .content_disposition()
                .and_then(|cd| cd.get_filename())
                .unwrap_or("")
                .to_string();
            let content_type = field.content_type().map(|mime| mime.to_string());
            let mut bytes: Vec<u8> = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(multipart_error)? {
                if bytes.len() + chunk.len() > max_file_bytes {
                    return Err(ServiceError::ValidationError(format!(
                        "{file_label} file must be at most {max_file_bytes} bytes"
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            file = Some(UploadedFile {
                filename,
                content_type,
                bytes,
            });
        } else {
            let mut value: Vec<u8> = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(multipart_error)? {
                if value.len() + chunk.len() > MAX_TEXT_FIELD_BYTES {
                    return Err(ServiceError::ValidationError(format!(
                        "form field {name} is too large"
                    )));
                }
                value.extend_from_slice(&chunk);
            }
            let text = String::from_utf8(value).map_err(|_| {
                ServiceError::ValidationError(format!("form field {name} is not valid UTF-8"))
            })?;
            fields.insert(name, text);
        }
    }

    Ok((fields, file))
}

fn multipart_error(err: actix_multipart::MultipartError) -> ServiceError {
    ServiceError::BadRequest(format!("invalid multipart form: {err}"))
}
