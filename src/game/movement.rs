// src/game/movement.rs
//
// 🧭 点击移动模块：把"目标世界坐标"换算成"该往屏幕上哪个像素点一下"。
//
// 前提条件（已经跟使用者确认过，不满足这些前提这套算法就不成立）：
// 1. 镜头视角固定不变，永远是同一个角度、不会转动/旋转。
// 2. 角色始终固定显示在屏幕正中央（镜头跟随角色）。
// 3. 点击游戏画面某处，角色会自动寻路走过去。
//
// 🔄 这是第二版设计，替换掉了第一版"每次启动自动标定方向矩阵"的
// 方案。第一版理论上更通用(不管镜头什么角度都能测出来)，但代价是
// 运行时极其脆弱——标定要真的操作鼠标点好几次、每次都要等角色走完
// 停稳，任何一次测试点击失败(卡墙、压到怪物、坐标识别抖动)，整个
// 标定链条就可能测不出可靠的方向。实测下来，"几乎没动"这四个字背后
// 可能是好几种完全不同的原因(地形卡住/点在怪物身上/识别抖动)，日志
// 上看起来一样，排查成本很高。
//
// 现在换成更简单可靠的思路：镜头角度是固定的，"世界坐标方向 -> 屏幕
// 点击方向"这个映射关系本身也是固定不变的，没必要每次启动都冒险现场
// 测——由使用者自己肉眼确认一次"点右边，人物是不是真的往右走了"，
// 写进下面 `MovementConfig` 的 `invert_x`/`invert_y`/`swap_xy` 三个
// 开关就行，不需要再经历那一整套容易失败的自动标定流程。

use crate::game::actions::click_at;
use crate::game::state::GameInfo;
use enigo::Enigo;
use xcap::Window;

/// 世界坐标增量 -> 屏幕点击方向的转换配置。
///
/// ⚠️ 三个翻转开关需要你实测确认一次：让角色站在开阔地，看着代码里
/// `move_toward` 打印出的"目标方向"日志，对照角色实际往哪走，判断
/// 要不要翻转。最常见的"北上"俯视角一般是 `invert_x=false,
/// invert_y=false, swap_xy=false`(世界 X 增大 = 屏幕右，世界 Y 增大
/// = 屏幕下)，但也可能整个反过来，需要你实测确认。
#[derive(Debug, Clone, Copy)]
pub struct MovementConfig {
    /// 世界 X 增大方向是否要反过来(屏幕上其实应该往左点)
    pub invert_x: bool,
    /// 世界 Y 增大方向是否要反过来(屏幕上其实应该往上点)
    pub invert_y: bool,
    /// 世界坐标系是不是整个转了90度(X/Y 对应的屏幕方向要互换)
    pub swap_xy: bool,

    /// 🎯 下面几个半径比例，都是"占截图较短边一半"的比例，是用真实
    /// 游戏截图实测确认过的安全范围(20% 贴着角色本体包围盒，35% 是
    /// 兼顾"窄路地形不容易点空"和"点击距离不要太保守"选出来的默认
    /// 点击半径，具体见对话记录里的实测过程)。
    pub base_radius_frac: f64,
    pub min_radius_frac: f64,
    pub max_radius_frac: f64,

    /// 🛡️ 规避怪物用：把检测到的怪物名字框往四周/往下(角色本体通常
    /// 站在名字下方)扩出去多少像素，作为要绕开的"占用区域"。
    pub entity_avoid_side_pad_px: f64,
    pub entity_avoid_top_pad_px: f64,
    pub entity_avoid_body_extend_px: f64,
    /// 撞到怪物时，最多尝试绕开多少次(每次把点击方向旋转一个角度
    /// 重新试探)，超过这个次数还是没找到空地就用原始点位兜底点击。
    pub entity_avoid_max_attempts: u32,
    /// 每次绕开重试，点击方向旋转的角度(度)。
    pub entity_avoid_angle_step_deg: f64,
}

impl Default for MovementConfig {
    fn default() -> Self {
        MovementConfig {
            invert_x: false,
            invert_y: false,
            swap_xy: false,

            base_radius_frac: 0.35,
            min_radius_frac: 0.20,
            max_radius_frac: 0.35,

            entity_avoid_side_pad_px: 40.0,
            entity_avoid_top_pad_px: 20.0,
            entity_avoid_body_extend_px: 80.0,
            entity_avoid_max_attempts: 8,
            entity_avoid_angle_step_deg: 20.0,
        }
    }
}

/// 判定"已经走到目标点"的世界坐标容差。坐标数字识别本身有量化
/// 误差（整数像素级别),卡太死会导致永远差一点点、原地反复点击。
pub const ARRIVE_TOLERANCE: f64 = 6.0;

/// 把"截图像素坐标系下,相对屏幕中心的偏移量"换算成"该点击的系统
/// 绝对屏幕坐标"。跟 actions::click_button 换算按钮点击坐标是同一套
/// 逻辑（x/y 分开算缩放比例，参考 position_reader.rs 踩过的坑）。
fn compute_click_point(
    window: &Window,
    capture_w: u32,
    capture_h: u32,
    screen_offset: (f64, f64),
) -> Result<(i32, i32), String> {
    let logical_w = window
        .width()
        .map_err(|e| format!("拿不到窗口逻辑宽度: {}", e))?;
    let logical_h = window
        .height()
        .map_err(|e| format!("拿不到窗口逻辑高度: {}", e))?;
    let window_x = window
        .x()
        .map_err(|e| format!("拿不到窗口 x 坐标: {}", e))?;
    let window_y = window
        .y()
        .map_err(|e| format!("拿不到窗口 y 坐标: {}", e))?;

    if capture_w == 0 || capture_h == 0 {
        return Err("截图宽高为 0，帧数据异常".to_string());
    }

    let scale_x = logical_w as f64 / capture_w as f64;
    let scale_y = logical_h as f64 / capture_h as f64;

    let center_logical_x = logical_w as f64 / 2.0;
    let center_logical_y = logical_h as f64 / 2.0;

    let click_logical_x = center_logical_x + screen_offset.0 * scale_x;
    let click_logical_y = center_logical_y + screen_offset.1 * scale_y;

    let abs_x = window_x + click_logical_x.round() as i32;
    let abs_y = window_y + click_logical_y.round() as i32;

    Ok((abs_x, abs_y))
}

/// 世界坐标增量 -> 屏幕方向单位向量(应用 invert_x/invert_y/swap_xy 校准)。
fn world_delta_to_screen_unit_dir(dx: f64, dy: f64, cfg: &MovementConfig) -> (f64, f64) {
    let (mut sx, mut sy) = (dx, dy);
    if cfg.invert_x {
        sx = -sx;
    }
    if cfg.invert_y {
        sy = -sy;
    }
    if cfg.swap_xy {
        std::mem::swap(&mut sx, &mut sy);
    }
    let len = (sx * sx + sy * sy).sqrt().max(1e-6);
    (sx / len, sy / len)
}

/// 🛡️ 判断一个截图像素坐标点是不是落在"怪物占用区域"里——检测到的
/// 名字框(头顶飘字)本身，加上四周/往下(角色本体通常站在名字下方)
/// 扩出去的一圈安全边距。落在这个区域里的点，点击很可能被判定成
/// "选中目标"而不是"移动"，导致角色不动。
fn point_blocked_by_entity(
    x: f64,
    y: f64,
    monster_boxes: &[(i32, i32, i32, i32)],
    cfg: &MovementConfig,
) -> bool {
    let side = cfg.entity_avoid_side_pad_px;
    let top = cfg.entity_avoid_top_pad_px;
    let bottom = cfg.entity_avoid_body_extend_px;

    monster_boxes.iter().any(|&(bx, by, bw, bh)| {
        let min_x = bx as f64 - side;
        let max_x = (bx + bw) as f64 + side;
        let min_y = by as f64 - top;
        let max_y = (by + bh) as f64 + bottom;
        x >= min_x && x <= max_x && y >= min_y && y <= max_y
    })
}

/// 世界坐标增量 -> 截图像素坐标系下"相对屏幕中心的点击偏移量"，并
/// 规避"点在活着的怪物身上"。
///
/// `retry_offset` 通常对应调用方那边记录的"连续卡住次数"：如果上一次
/// 朝同一个方向点击之后角色坐标完全没变，调用方应该把这个值递增再
/// 传进来——不管这次有没有检测到怪物挡路，只要 `retry_offset > 0`
/// 就强制换一个跟上次不一样的候选点，用 `retry_offset` 错开旋转重试
/// 的起始角度，避免连续几次"卡住->重试"都点在同一个失败点上(比如
/// 挡路的东西没被怪物名字框检测覆盖到，比如宠物、其他玩家、尸体)。
fn direction_to_screen_offset(
    dx: f64,
    dy: f64,
    cfg: &MovementConfig,
    half_min: f64,
    monster_boxes: &[(i32, i32, i32, i32)],
    retry_offset: u32,
) -> (f64, f64) {
    let (ux, uy) = world_delta_to_screen_unit_dir(dx, dy, cfg);
    let base_radius = cfg.base_radius_frac * half_min;
    let base_offset = (ux * base_radius, uy * base_radius);

    let base_blocked = !monster_boxes.is_empty()
        && point_blocked_by_entity(base_offset.0, base_offset.1, monster_boxes, cfg);

    if retry_offset == 0 && !base_blocked {
        return base_offset;
    }

    if base_blocked {
        println!("⚠️  [寻路] 目标点撞到怪物名字框，尝试换个方向点击...");
    }

    let attempts = cfg.entity_avoid_max_attempts.max(1);
    for step in 0..attempts {
        let i = (step + retry_offset) % attempts + 1;
        for sign in [1.0_f64, -1.0] {
            let angle_deg = sign * cfg.entity_avoid_angle_step_deg * i as f64;
            let angle_rad = angle_deg.to_radians();
            let (sin_a, cos_a) = angle_rad.sin_cos();
            let rux = ux * cos_a - uy * sin_a;
            let ruy = ux * sin_a + uy * cos_a;
            let candidate = (rux * base_radius, ruy * base_radius);
            if monster_boxes.is_empty()
                || !point_blocked_by_entity(candidate.0, candidate.1, monster_boxes, cfg)
            {
                return candidate;
            }
        }
    }

    let radius_options = [
        cfg.min_radius_frac,
        cfg.max_radius_frac,
        (cfg.min_radius_frac + cfg.base_radius_frac) / 2.0,
        (cfg.base_radius_frac + cfg.max_radius_frac) / 2.0,
    ];
    for k in 0..radius_options.len() {
        let frac = radius_options[(k + retry_offset as usize) % radius_options.len()];
        let radius = frac * half_min;
        let candidate = (ux * radius, uy * radius);
        if monster_boxes.is_empty()
            || !point_blocked_by_entity(candidate.0, candidate.1, monster_boxes, cfg)
        {
            return candidate;
        }
    }

    println!("⚠️  [寻路] 尝试多次仍未换到空地，使用原始方向兜底点击");
    base_offset
}

/// 🚶 朝目标世界坐标点一下(单次点击，不是走到底为止)。调用方负责
/// 循环调用 + 判断是否已经进入 ARRIVE_TOLERANCE 范围。
///
/// `retry_offset` 见 `direction_to_screen_offset` 的说明——调用方在
/// 检测到"连续几帧坐标没变"时应该递增这个值再传进来，触发换方向重试。
pub fn move_toward(
    enigo: &mut Enigo,
    window: &Window,
    info: &GameInfo,
    target: (i32, i32),
    cfg: &MovementConfig,
    retry_offset: u32,
) -> Result<(), String> {
    let cur = info
        .player_position
        .ok_or_else(|| "当前没有有效坐标".to_string())?;

    let world_dx = (target.0 - cur.0) as f64;
    let world_dy = (target.1 - cur.1) as f64;

    let half_min = info.capture_width.min(info.capture_height) as f64 / 2.0;
    let offset = direction_to_screen_offset(
        world_dx,
        world_dy,
        cfg,
        half_min,
        &info.monster_boxes,
        retry_offset,
    );

    let click_point = compute_click_point(window, info.capture_width, info.capture_height, offset)?;

    click_at(
        enigo,
        click_point.0,
        click_point.1,
        &format!("寻路移动 -> 目标({},{})", target.0, target.1),
    );

    Ok(())
}
