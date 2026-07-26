use std::time::Instant;

/// 游戏当前状态快照
#[derive(Debug, Clone)]
pub struct GameInfo {
    pub map_name: String,
    pub player_position: Option<(i32, i32)>,
    pub hp_percent: u8,
    pub monsters: Vec<String>,
    pub items: Vec<String>,
    pub updated_at: Option<Instant>,
}

impl Default for GameInfo {
    fn default() -> Self {
        GameInfo {
            map_name: String::new(),
            player_position: None,
            hp_percent: 100,
            monsters: Vec::new(),
            items: Vec::new(),
            updated_at: None,
        }
    }
}

// TODO: 之后按需要补充,比如:
// - GameInfo::new(...) 构造函数
// - 从原始字节 / 内存读取解析成 GameInfo 的方法
// - 校验字段合法性的方法
