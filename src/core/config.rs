use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;
use std::env;
use std::str::FromStr;
use std::{collections::HashMap, path::Path};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncReadExt,
};
use crate::utils;
use toml::Value;

#[derive(Deserialize, Serialize, Type, Clone)]
#[zvariant(signature = "dict")]
pub struct Config {
    // /!\ After changing properties, change also the updater in the impl of this struct
    pub default_directory: String,
    pub temp_directory: String,
    pub user_agent: String,
    pub categories: HashMap<String, Category>,
    pub max_sim_downloads: u16,
}

#[derive(Deserialize, Serialize, Type, Clone)]
#[zvariant(signature = "dict")]
pub struct Category {
    pub extensions: Vec<String>,
    pub directory: String,
}

impl Config {
    pub fn update_from_map(&mut self, config: &str) -> Result<(), toml::de::Error> {
        let parsed_config = config.parse::<toml::Table>()?;

        if let Some(value) = parsed_config.get("default_directory") {
            self.default_directory = String::from_str(value.as_str().unwrap()).unwrap();
        }
        if let Some(value) = parsed_config.get("temp_directory") {
            self.temp_directory = String::from_str(value.as_str().unwrap()).unwrap();
        }
        if let Some(value) = parsed_config.get("user_agent") {
            self.user_agent = String::from_str(value.as_str().unwrap()).unwrap();
        }
        if let Some(value) = parsed_config.get("categories") {
            let mut categories: HashMap<String, Category> = HashMap::new();
            let parsed_categories = value.as_table().unwrap();
            parsed_categories.keys().for_each(|key| {
                let value = parsed_categories.get(key).unwrap();
                let value = toml::Value::try_into::<Category>(value.clone()).unwrap();
                categories.insert(key.clone(), value);
            });
            self.categories = categories;
        }
        if let Some(value) = parsed_config.get("max_sim_downloads") {
            self.max_sim_downloads = u16::try_from(value.as_integer().unwrap()).unwrap();
        }
        Ok(())
    }
}

const USER_CONFIG_PATH: &str = "~/.config/flowd/config.toml";
const ROOT_CONFIG_PATH: &str = "/etc/flowd/config.toml";
const DEFAULT_CONFIG_PATH: &str = "/usr/share/flowd/config/config.toml";

pub async fn get_config() -> Config {
    let expanded_user_config_path = utils::path::expand(USER_CONFIG_PATH);

    // Get default config
    let default_config_toml = fs::read_to_string(DEFAULT_CONFIG_PATH).await.unwrap();
    let mut config: Config = toml::from_str(&default_config_toml).unwrap();

    // Update with system config
    if Path::new(ROOT_CONFIG_PATH).exists() {
        let system_config = fs::read_to_string(ROOT_CONFIG_PATH).await.unwrap();
        config.update_from_map(&system_config).unwrap_or_else(|error| {
            log::error!("{error}");
        });
    }

    // Update with user config if available
    if Path::new(&expanded_user_config_path).exists() {
        let system_config = fs::read_to_string(&expanded_user_config_path).await.unwrap();
        config.update_from_map(&system_config).unwrap_or_else(|error| {
            log::error!("{error}");
        });
    }

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