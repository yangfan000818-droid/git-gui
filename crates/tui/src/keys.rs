//! 方向键归一化。
//!
//! event_loop 把方向键映射成下列控制字符再走 `dispatch`,各视图按需把它们
//! 绑定为 j/k(上下)或左右动作。用控制字符是关键:文本输入模式(分支名/
//! stash 名/提交信息)只接收非控制字符,因而会自动忽略方向键,不会误当文本。
pub const UP: char = '\u{10}';
pub const DOWN: char = '\u{0e}';
pub const LEFT: char = '\u{02}';
pub const RIGHT: char = '\u{06}';
