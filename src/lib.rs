//! weread-notes 库——CLI 与服务器共享的网关客户端、编排与 Markdown 渲染。
//!
//! - [`client`]：官方 Agent Gateway 协议实现（async）与 [`client::GatewayApi`] 抽象
//! - [`fetch`]：多接口翻页/合并编排
//! - [`markdown`]：读书笔记 Markdown 渲染
//! - [`commands`]：CLI 命令（薄 IO 层）
//! - [`server`]：axum 服务器（设计见 docs/weread-server-design.md）

pub mod client;
pub mod commands;
pub mod fetch;
pub mod markdown;
pub mod server;
