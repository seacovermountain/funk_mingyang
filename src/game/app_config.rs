// src/game/app_config.rs
//
// 读取项目根目录的 config.toml，把怪物/物品白名单转成 HashSet
// （给 OCR 白名单模糊匹配用，查找效率也比 Vec 高）。

use serde::Deserialize;
use std::collections::HashSet;
use std::fs;

#[derive(Debug, Deserialize)]
struct RawConfig {
    hunting: HuntingSection,
    loot: LootSection,
}

#[derive(Debug, Deserialize)]
struct HuntingSection {
    target_monsters: Vec<String>,
    hp_protect_line: u8,
}

#[derive(Debug, Deserialize)]
struct LootSection {
    target_items: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub target_monsters: HashSet<String>,
    pub target_items: HashSet<String>,
    pub hp_protect_line: u8,
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self, String> {
        let raw =
            fs::read_to_string(path).map_err(|e| format!("读取配置文件失败 {}: {}", path, e))?;
        let parsed: RawConfig =
            toml::from_str(&raw).map_err(|e| format!("解析配置文件失败 {}: {}", path, e))?;

        Ok(AppConfig {
            target_monsters: parsed.hunting.target_monsters.into_iter().collect(),
            target_items: parsed.loot.target_items.into_iter().collect(),
            hp_protect_line: parsed.hunting.hp_protect_line,
        })
    }
}
