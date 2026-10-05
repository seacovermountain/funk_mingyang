// src/game/app_config.rs
//
// 读取项目根目录的 config.toml，把怪物/物品白名单转成 HashSet
// （给 OCR 白名单模糊匹配用，查找效率也比 Vec 高）。

use serde::Deserialize;
use std::collections::HashSet;

/// config.toml 的内容在编译时嵌入进可执行文件——部署只需要一个可执行
/// 文件，不用再带着这个文件到处走。代价是：改怪物/物品白名单、护体
/// 血线这些配置，都需要重新 `cargo build` 才能生效，不能像外部文件
/// 那样改完直接生效。
const CONFIG_TOML_TEXT: &str = include_str!("../../config.toml");

#[derive(Debug, Deserialize)]
struct RawConfig {
    hunting: HuntingSection,
    loot: LootSection,
    /// 巡逻路线是可选的——没配置的时候就是空数组，寻路模块会原地
    /// 空转（不会 panic），方便还没配置巡逻路线的人先跑通拾取/攻击。
    #[serde(default)]
    patrol: Vec<RawPatrolRoute>,
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

#[derive(Debug, Deserialize)]
struct RawPatrolRoute {
    map: String,
    points: Vec<(i32, i32)>,
}

/// 一条地图的固定巡逻路线：地图名字 + 一串按顺序巡逻的坐标点，
/// 走完最后一个点会循环回第一个点。
#[derive(Debug, Clone)]
pub struct PatrolRoute {
    pub map: String,
    pub points: Vec<(i32, i32)>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub target_monsters: HashSet<String>,
    pub target_items: HashSet<String>,
    pub hp_protect_line: u8,
    pub patrol_routes: Vec<PatrolRoute>,
}

impl AppConfig {
    pub fn load() -> Result<Self, String> {
        let parsed: RawConfig =
            toml::from_str(CONFIG_TOML_TEXT).map_err(|e| format!("解析内置配置失败: {}", e))?;

        let patrol_routes = parsed
            .patrol
            .into_iter()
            .map(|r| PatrolRoute {
                map: r.map,
                points: r.points,
            })
            .collect();

        Ok(AppConfig {
            target_monsters: parsed.hunting.target_monsters.into_iter().collect(),
            target_items: parsed.loot.target_items.into_iter().collect(),
            hp_protect_line: parsed.hunting.hp_protect_line,
            patrol_routes,
        })
    }

    /// 按地图名字查这张地图配置的巡逻路线，没配置就是 None。
    pub fn patrol_route_for(&self, map_name: &str) -> Option<&PatrolRoute> {
        self.patrol_routes.iter().find(|r| r.map == map_name)
    }
}
