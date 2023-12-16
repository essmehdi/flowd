use serde::{Deserialize, Serialize};
use std::env;
use std::{collections::HashMap, path::Path};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncReadExt,
};

use crate::utils;

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub default_directory: String,
    pub temp_directory: String,
    pub user_agent: String,
    pub categories: HashMap<String, Category>,
    pub max_sim_downloads: u16,
}

#[derive(Deserialize, Serialize)]
pub struct Category {
    pub extensions: Vec<String>,
    pub directory: String,
}

const USER_CONFIG_PATH: &str = "~/.config/flow/config.toml";
const DEFAULT_CONFIG_PATH: &str = "/etc/flow/config.toml";

async fn init_user_config() {
    let user_config_path = utils::path::expand(USER_CONFIG_PATH);
    let path = Path::new(&user_config_path);
    
    
    if fs::try_exists(path).await.unwrap() {
        return;
    }

    let path_parent = path.parent().unwrap();
    if !fs::try_exists(path_parent).await.unwrap() {
        fs::create_dir(path_parent).await.unwrap();
    }

    fs::copy(DEFAULT_CONFIG_PATH, user_config_path)
        .await
        .unwrap();
}

pub async fn get_config() -> Config {
    let is_root = match env::var("USER") {
        Ok(user) => user == "root",
        Err(_) => false,
    };

    let expanded_user_config_path = utils::path::expand(USER_CONFIG_PATH);
    let config_path = if is_root {
        DEFAULT_CONFIG_PATH
    } else {
        init_user_config().await;
        &expanded_user_config_path
    };

    let mut file = OpenOptions::new()
        .read(true)
        .open(config_path)
        .await
        .unwrap();

    let mut config_toml = String::new();
    file.read_to_string(&mut config_toml).await.unwrap();

    let config = toml::from_str(&config_toml).unwrap();

    process_config(config)
}

pub async fn get_default_directory() -> String {
    let config = get_config().await;
    utils::path::expand(&config.default_directory)
}

pub async fn get_categories() -> HashMap<String, Category> {
    let config = get_config().await;
    config.categories
}

fn process_config(mut config: Config) -> Config {
    config.default_directory = utils::path::expand(&config.default_directory);
    config.temp_directory = utils::path::expand(&config.temp_directory);
    config
}
