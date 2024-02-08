use std::path::Path;

use mime_guess::get_mime_extensions_str;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::header::HeaderMap;
use tokio::fs;

use crate::{core::config::Config, utils::{self, path::expand}};

use super::FileInfo;

/// This function is used to extract file info from headers and fallbacks to url
///
/// # Arguments
///
/// * `url` - The url of the file
/// * `headers` - The headers of the response
///
/// # Returns
///
/// * `FileInfo` - The file info
pub fn get_file_info_from_headers(url: &str, headers: &HeaderMap) -> FileInfo {
    // Get file content type
    let content_type = headers.get("content-type").and_then(|ct| {
        ct.to_str()
            .ok()
            .and_then(|ct| Some(ct.split(";").collect::<Vec<&str>>()[0].to_string()))
    });

    // Get file name if available
    let file_name_from_header = headers.get("content-disposition").and_then(|ct| {
        ct.to_str()
            .ok()
            .and_then(|ct| ct.find("filename=").and_then(|i| Some(i + 9)))
            .and_then(|i| Some(ct.to_str().unwrap()[i..ct.len() - 1].to_string()))
    });

    // Check if file is resumable
    let resumable = headers
        .get("accept-ranges")
        .and_then(|ar| {
            ar.to_str()
                .ok()
                .and_then(|ar| Some(ar.to_lowercase() == "bytes"))
        })
        .unwrap_or(false);

    let ct_extension = match &content_type {
        Some(ct) => {
            get_mime_extensions_str(&ct)
                .and_then(|ext| {
                    if ext.len() > 0 {
                        Some(ext[0])
                    } else { 
                        None 
                    }
                })
                .unwrap_or("")
        },
        None => ""
    };

    // Deduce file name
    let file_name = match file_name_from_header {
        None => {
            let mut name = url.split("/").last().unwrap().to_string();
            if name.is_empty() {
                name = "download".to_string();
            }
            if name.ends_with(&ct_extension) {
                name
            } else {
                format!("{}.{}", name, ct_extension)
            }
        }
        Some(name) => name,
    };

    return FileInfo {
        file_name,
        content_length: headers.get("content-length").and_then(|ct_len| {
            ct_len
                .to_str()
                .ok()
                .and_then(|ct_len| ct_len.parse::<u64>().ok())
        }),
        content_type,
        resumable,
    };
}

/// This function detects the file category and gets the output file path according to config
///
/// # Arguments
///
/// * `file_info` - The file info
/// * `config` - The config
///
/// # Returns
///
/// * `String` - The output file path
pub async fn get_output_file_path(file_info: &FileInfo, config: &Config) -> String {
    let file_extension = file_info
        .file_name
        .split(".")
        .last()
        .and_then(|ext| Some(ext))
        .unwrap_or("");

    let mut file_directory = "".to_string();
    if file_extension != "" {
        'outer: for (_, category) in &config.categories {
            let mut extensions_iter = category.extensions.iter();
            while let Some(extension) = extensions_iter.next() {
                if file_info.file_name.ends_with(extension) {
                    file_directory = category.directory.clone();
                    break 'outer;
                }
            }
        }
    }
    if file_directory.len() == 0 {
        file_directory = config.default_directory.clone();
    }

    let file_path = Path::new(&file_directory).join(&file_info.file_name);
    utils::path::expand(file_path.to_str().unwrap())
}

pub async fn get_temp_file(config: &Config) -> String {
    let expanded_temp_directory = expand(&config.temp_directory);
    let temp_directory = Path::new(&expanded_temp_directory);

    if !Path::new(&temp_directory).is_dir() {
        fs::create_dir_all(&temp_directory).await.unwrap();
    }

    let random_file_name = rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(10)
        .map(char::from)
        .collect::<String>();

    let temp_file = temp_directory
        .join(random_file_name)
        .to_str()
        .unwrap()
        .to_string();

    temp_file
}

/// This function is used to check for file name conflicts
pub fn get_conflict_free_file_path(file_path: &str) -> String {
    // TODO: Fix complex extensions (e.g.: .tar.gz)
    let mut file_path = file_path.to_string();
    let mut i = 1;
    while Path::new(&file_path).exists() {
        let file_name = Path::new(&file_path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let file_extension = Path::new(&file_path)
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let file_name_without_extension = file_name.replace(&format!(".{}", &file_extension), "");
        file_path = file_path.replace(
            &format!("{}.{}", &file_name_without_extension, &file_extension),
            &format!("{} ({})", &file_name_without_extension, i),
        );
        if file_extension.len() > 0 {
            file_path = format!("{}.{}", file_path, file_extension);
        }
        i += 1;
    }
    file_path
}