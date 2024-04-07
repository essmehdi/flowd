use std::path::Path;

use mime_guess::get_mime_extensions_str;
use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use reqwest::{header::HeaderMap, Url};
use tokio::{fs::{self, OpenOptions}, io};
use urlencoding::decode;

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
            .and_then(|ct| {
                if let Some(index) = ct.find("filename=\"") {
                    Some(index + 10)
                } else {
                    if let Some(index) = ct.find("filename=") {
                        Some(index + 9)
                    } else {
                        None
                    }
                }
            }).and_then(|i| Some(ct.to_str().unwrap()[i..ct.len() - 1].to_string()))
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
            let last_url_segment = 
                Url::parse(url)
                    .unwrap()
                    .path_segments()
                    .and_then(|segments| {
                        let last_segment = segments.last();
                        if let Some(seg) = last_segment {
                            if seg == "" {
                                return None;
                            }
                            last_segment
                        } else {
                            None
                        }
                    })
                    .unwrap_or("download")
                    .to_string();
            if last_url_segment.ends_with(&ct_extension) {
                last_url_segment
            } else {
                format!("{}.{}", last_url_segment, ct_extension)
            }
        }
        Some(name) => name,
    };

    let file_name  = decode(&file_name).unwrap().into_owned();

    FileInfo {
        file_name,
        content_length: headers.get("content-length").and_then(|ct_len| {
            ct_len
                .to_str()
                .ok()
                .and_then(|ct_len| ct_len.parse::<u64>().ok())
        }),
        content_type,
        resumable,
    }
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

pub fn get_conflict_free_file_path(file_path: &str) -> String {

    log::debug!("Getting conflict free file path for {}", file_path);

    let special_extensions = [
        ".tar.gz",
        ".tar.xz",
        ".tar.bz2",
        ".tar.lz",
        ".tar.lzma",
        ".tar.lzo",
        ".tar.lz4",
        ".tar.Z",
    ];

    let file_path = Path::new(file_path);
    let file_path_parent = file_path.parent().unwrap();

    let mut special_extension = "";
    let ends_with_special_extension = special_extensions.iter().any(|ext| {
        special_extension = ext;
        file_path.to_str().unwrap().ends_with(ext)
    });
    
    let mut file_path_split = if ends_with_special_extension {
        let file_stem = file_path.to_str().unwrap().split(special_extension).collect::<Vec<&str>>();
        let file_stem = file_stem[0];
        vec![file_stem, &special_extension[1..]]
    } else {
        vec![file_path.file_stem().unwrap().to_str().unwrap(), file_path.extension().unwrap().to_str().unwrap()]
    };

    if Regex::new(r" \(\d+\)$").unwrap().is_match(file_path_split[0]) {
        let conflict_number_index = file_path_split[0].rfind(" (").unwrap();
        file_path_split[0] = &file_path_split[0][..conflict_number_index];
    }

    let mut i = 1;
    
    let mut new_file_path = file_path_parent.join(file_path_split.join("."));
    while new_file_path.exists() {
        let file_stem = file_path_split[0];
        let file_extension = file_path_split[1];
        new_file_path = file_path_parent.join(format!("{} ({}).{}", file_stem, i, file_extension));
        i += 1;
    }

    new_file_path.to_str().unwrap().to_string()
}

pub async fn empty_temp_file(temp_file_path: &str) -> Result<(), io::Error> {
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp_file_path)
        .await
        .map(|_| ())
        .map_err(|e| {
            log::warn!("Could not empty temp file: {e}");
            e
        })
}