//! 视图滚动辅助。

/// 计算纵向滚动偏移,使第 `cursor` 行(0 基)落在高度 `viewport` 的可视区内。
/// 无状态:向下越界时把光标顶到末行,向上回退时随之收缩。`viewport` 传去掉
/// 边框后的内容高度(通常为 `area.height - 2`)。
pub fn follow(cursor: usize, viewport: u16) -> u16 {
    let visible = viewport as usize;
    if visible == 0 {
        return 0;
    }
    cursor.saturating_sub(visible - 1) as u16
}
