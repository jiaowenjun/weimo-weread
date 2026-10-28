//! weimo-weread 库——服务器使用的网关客户端与编排。
//!
//! - [`client`]：官方 Agent Gateway 协议实现（async）与 [`client::GatewayApi`] 抽象
//! - [`fetch`]：多接口翻页/合并编排
//! - [`server`]：axum 服务器（设计见 docs/weread-server-design.md）

pub mod client;
pub mod fetch;
pub mod server;
