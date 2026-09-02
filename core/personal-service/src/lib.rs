// audience: external
// # personal-service
//
// 该库在应用进程内提供 loopback HTTP 服务和 SQLite 持久化. 服务仅绑定 127.0.0.1.
// 远端配置只转发 CN API 请求. 管理接口始终在本机处理.
// CN 资产默认位于 root/cdn/cn, 也可以由启动方显式提供其他只读根目录.
// 每个远端配置单独保存设备和 viewer 映射. 远端身份不创建本地账号或玩家快照.
// 本地存档导出不包含账号会话或管理凭据.
// 本地存档加密密钥只保存在个人服务 SQLite 中, 密文导出不包含密钥.
// 密文存档同步只向 HTTPS 或 loopback 目标发送远端登录凭据.
// 每次状态变更使用 SQLite 原子语句提交. flush 在应用暂停前截断 WAL.

mod activity_calendar;
mod activity_catalog;
mod activity_projection;
mod ai_teams;
mod cn;
mod cn_activity;
mod cn_asset;
mod cn_asset_files;
mod cn_auxiliary;
mod cn_battle;
mod cn_battle_assets;
mod cn_battle_rewards;
mod cn_battle_state;
mod cn_box_gacha;
mod cn_character;
mod cn_character_reward;
mod cn_episode_trial_reading;
mod cn_equipment;
mod cn_ex_boost;
mod cn_exchange;
mod cn_expod;
mod cn_gacha;
mod cn_mail;
mod cn_mana;
mod cn_mission;
mod cn_msgpack;
mod cn_multi;
mod cn_multi_special_exchange;
mod cn_multiplayer;
mod cn_news;
mod cn_option;
mod cn_optional_exchange;
mod cn_party;
mod cn_party_group;
mod cn_pass_card;
mod cn_payment;
mod cn_player;
mod cn_profile;
mod cn_quest;
mod cn_reference_read;
mod cn_reference_state_misc;
mod cn_shop;
mod cn_stamina;
mod cn_story;
mod cn_tutorial;
mod database;
mod error;
mod ffi;
mod gameplay_settings;
mod http;
mod local_saves;
mod management;
mod management_web;
mod player_web;
mod portable_save;
mod remote_forward;
mod sdk_compat;
mod server_profiles;
mod service;
mod virtual_time;

pub use error::PersonalServiceError;
pub use ffi::{
    starpoint_personal_service_copy_management_token, starpoint_personal_service_flush,
    starpoint_personal_service_is_running, starpoint_personal_service_port,
    starpoint_personal_service_start, starpoint_personal_service_start_with_cdn_root,
    starpoint_personal_service_stop, StarpointPersonalService,
};
pub use service::{PersonalService, PersonalServiceOptions};
