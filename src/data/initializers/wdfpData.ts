// audience: internal
// # wdfp-database-schema
//
// 该模块使用可重复执行的建表语句初始化 Node 游戏数据库.
// 新表同时适用于空数据库和已有 SQLite 文件.

import { Database } from "better-sqlite3";


export default function init(
    database: Database,
    exists: Boolean
) {
    // initialize the database

    // create players table
    database.prepare(`CREATE TABLE IF NOT EXISTS accounts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        app_id TEXT NOT NULL,
        first_login_time DATE NOT NULL,
        idp_alias TEXT NOT NULL,
        idp_code TEXT NOT NULL,
        idp_id TEXT NOT NULL,
        reg_time DATE NOT NULL,
        last_login_time DATE NOT NULL,
        status TEXT NOT NULL
    )`).run()

    // create zat session table
    database.prepare(`CREATE TABLE IF NOT EXISTS sessions (
        token TEXT PRIMARY KEY NOT NULL,
        account_id INTEGER NOT NULL,
        expires DATE NOT NULL,
        type INTEGER NOT NULL,
        FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
    )`).run()

    // create players table
    database.prepare(`CREATE TABLE IF NOT EXISTS players (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        stamina INTEGER NOT NULL,
        stamina_heal_time INTEGER NOT NULL,
        boost_point INTEGER NOT NULL,
        boss_boost_point INTEGER NOT NULL,
        transition_state INTEGER NOT NULL,
        role INTEGER NOT NULL,
        name TEXT NOT NULL,
        last_login_time DATE NOT NULL,
        comment TEXT NOT NULL,
        vmoney INTEGER NOT NULL,
        free_vmoney INTEGER NOT NULL,
        rank_point INTEGER NOT NULL,
        star_crumb INTEGER NOT NULL,
        bond_token INTEGER NOT NULL,
        exp_pool INTEGER NOT NULL,
        exp_pooled_time INTEGER NOT NULL,
        leader_character_id INTEGER NOT NULL,
        party_slot INTEGER NOT NULL,
        degree_id INTEGER NOT NULL,
        birth INTEGER NOT NULL,
        free_mana INTEGER NOT NULL,
        paid_mana INTEGER NOT NULL,
        enable_auto_3x INTEGER NOT NULL,
        account_id INTEGER NOT NULL,
        tutorial_step INTEGER,
        tutorial_skip_flag INTEGER,
        FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
    )`).run();

    // //// 保存账号当前激活的玩家存档 [@x380kkm 2026-07-27] ////
    database.prepare(`CREATE UNIQUE INDEX IF NOT EXISTS players_account_id_id
        ON players (account_id, id)
    `).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS account_active_players (
        account_id INTEGER PRIMARY KEY NOT NULL,
        player_id INTEGER NOT NULL,
        FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
        FOREIGN KEY (account_id, player_id) REFERENCES players (account_id, id) ON DELETE CASCADE
    )`).run()

    database.prepare(`INSERT OR IGNORE INTO account_active_players (account_id, player_id)
        SELECT account_id, MIN(id)
        FROM players
        GROUP BY account_id
    `).run()
    // //// /保存账号当前激活的玩家存档 ////

    // //// 保存不可变存档 revision 和当前指针 [@x380kkm 2026-07-27] ////
    database.prepare(`CREATE TABLE IF NOT EXISTS player_save_revisions (
        id TEXT PRIMARY KEY NOT NULL,
        player_id INTEGER NOT NULL,
        parent_revision_id TEXT,
        payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64),
        data_json TEXT NOT NULL,
        label TEXT NOT NULL,
        created_at TEXT NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
        FOREIGN KEY (parent_revision_id) REFERENCES player_save_revisions (id) ON DELETE RESTRICT
    )`).run()

    database.prepare(`CREATE INDEX IF NOT EXISTS player_save_revisions_player_created
        ON player_save_revisions (player_id, created_at DESC, id DESC)
    `).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS player_save_heads (
        player_id INTEGER PRIMARY KEY NOT NULL,
        revision_id TEXT NOT NULL UNIQUE,
        updated_at TEXT NOT NULL,
        FOREIGN KEY (revision_id) REFERENCES player_save_revisions (id) ON DELETE RESTRICT
    )`).run()

    database.prepare(`CREATE TRIGGER IF NOT EXISTS player_save_revisions_immutable
        BEFORE UPDATE OF id, player_id, parent_revision_id, payload_sha256, data_json, label, created_at
        ON player_save_revisions
        BEGIN
            SELECT RAISE(ABORT, 'save revisions are immutable');
        END
    `).run()
    // //// /保存不可变存档 revision 和当前指针 ////

    // //// 保存可移植存档的来源快照和导入基线 [@x380kkm 2026-08-03] ////
    database.prepare(`CREATE TABLE IF NOT EXISTS player_portable_snapshots (
        player_id INTEGER PRIMARY KEY NOT NULL,
        source_json TEXT NOT NULL,
        baseline_json TEXT NOT NULL,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()
    // //// /保存可移植存档的来源快照和导入基线 ////

    // //// 保存完整服务器的跨实例槽位绑定 [@x380kkm 2026-08-04] ////
    database.prepare(`CREATE TABLE IF NOT EXISTS server_transfer_bindings (
        id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
        source_player_id INTEGER NOT NULL,
        target_base_url TEXT NOT NULL CHECK (length(target_base_url) BETWEEN 1 AND 2048),
        target_instance_id TEXT NOT NULL CHECK (length(target_instance_id) = 32),
        target_shell_id TEXT NOT NULL CHECK (length(target_shell_id) BETWEEN 1 AND 128),
        target_player_id INTEGER NOT NULL CHECK (target_player_id > 0),
        target_token TEXT NOT NULL CHECK (length(target_token) BETWEEN 1 AND 512),
        upload_mode TEXT NOT NULL CHECK (upload_mode IN ('manual', 'interval')),
        pull_mode TEXT NOT NULL CHECK (pull_mode IN ('manual', 'interval')),
        conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('local_wins', 'remote_wins', 'ask')),
        interval_seconds INTEGER NOT NULL CHECK (interval_seconds BETWEEN 1 AND 2592000),
        enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
        last_common_etag TEXT CHECK (last_common_etag IS NULL OR length(last_common_etag) = 64),
        last_source_etag TEXT CHECK (last_source_etag IS NULL OR length(last_source_etag) = 64),
        last_target_etag TEXT CHECK (last_target_etag IS NULL OR length(last_target_etag) = 64),
        pending_direction TEXT NOT NULL CHECK (pending_direction IN ('none', 'upload', 'pull', 'conflict')),
        next_run_at TEXT NOT NULL,
        last_synced_at TEXT,
        last_error TEXT,
        revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE (source_player_id, target_instance_id, target_player_id)
    )`).run()

    database.prepare(`CREATE INDEX IF NOT EXISTS server_transfer_bindings_due
        ON server_transfer_bindings (enabled, next_run_at, id)
    `).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS player_data_replacement_guards (
        player_id INTEGER PRIMARY KEY NOT NULL
    )`).run()

    database.prepare(`CREATE TRIGGER IF NOT EXISTS server_transfer_bindings_block_player_delete
        BEFORE DELETE ON players
        WHEN EXISTS (
            SELECT 1 FROM server_transfer_bindings WHERE source_player_id = OLD.id
        ) AND NOT EXISTS (
            SELECT 1 FROM player_data_replacement_guards WHERE player_id = OLD.id
        )
        BEGIN
            SELECT RAISE(ABORT, 'server transfer binding blocks player deletion');
        END
    `).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS server_transfer_conflicts (
        id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
        binding_id TEXT NOT NULL,
        source_revision_id TEXT NOT NULL,
        source_etag TEXT NOT NULL CHECK (length(source_etag) = 64),
        target_revision_id TEXT,
        target_etag TEXT NOT NULL CHECK (length(target_etag) = 64),
        detected_at TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN (
            'open', 'resolved_local_wins', 'resolved_remote_wins', 'resolved_keep_both'
        )),
        resolved_at TEXT,
        FOREIGN KEY (binding_id) REFERENCES server_transfer_bindings (id) ON DELETE CASCADE,
        FOREIGN KEY (source_revision_id) REFERENCES player_save_revisions (id) ON DELETE RESTRICT
    )`).run()

    database.prepare(`CREATE UNIQUE INDEX IF NOT EXISTS server_transfer_conflicts_one_open
        ON server_transfer_conflicts (binding_id) WHERE status = 'open'
    `).run()
    // //// /保存完整服务器的跨实例槽位绑定 ////

    database.prepare(`CREATE TABLE IF NOT EXISTS players_options (
        key TEXT NOT NULL,
        value INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (key, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_triggered_tutorials (
        id INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_cleared_regular_missions (
        id INTEGER NOT NULL,
        value INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_mission_counters (
        pattern TEXT NOT NULL,
        value INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (pattern, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_items (
        id INTEGER NOT NULL,
        amount INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS daily_challenge_point_list_entries (
        id INTEGER NOT NULL,
        point INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS daily_challenge_point_list_campaigns (
        campaign_id INTEGER NOT NULL,
        additional_point INTEGER NOT NULL,
        list_entry_id INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (player_id, campaign_id, list_entry_id),
        FOREIGN KEY (list_entry_id, player_id) REFERENCES daily_challenge_point_list_entries (id, player_id) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_characters (
        id INTEGER NOT NULL,
        entry_count INTEGER NOT NULL,
        evolution_level INTEGER NOT NULL,
        over_limit_step INTEGER NOT NULL,
        protection INTEGER NOT NULL,
        join_time DATE NOT NULL,
        update_time DATE NOT NULL,
        exp INTEGER NOT NULL,
        stack INTEGER NOT NULL,
        mana_board_index INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        ex_boost_status_id INTEGER,
        ex_boost_ability_id_list TEXT,
        illustration_settings TEXT,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_characters_bond_tokens (
        mana_board_index INTEGER NOT NULL,
        status INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        character_id INTEGER NOT NULL,
        PRIMARY KEY (mana_board_index, player_id, character_id),
        FOREIGN KEY (character_id, player_id) REFERENCES players_characters (id, player_id) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_characters_mana_nodes (
        value INTEGER NOT NULL,
        character_id INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (value, character_id, player_id),
        FOREIGN KEY (character_id, player_id) REFERENCES players_characters (id, player_id) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_party_groups (
        id INTEGER NOT NULL,
        color_id INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        category INTEGER NOT NULL,
        PRIMARY KEY (id, player_id, category),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_parties (
        slot INTEGER NOT NULL,
        name TEXT NOT NULL,
        character_id_1 INTEGER,
        character_id_2 INTEGER,
        character_id_3 INTEGER,
        unison_character_1 INTEGER,
        unison_character_2 INTEGER,
        unison_character_3 INTEGER,
        equipment_1 INTEGER,
        equipment_2 INTEGER,
        equipment_3 INTEGER,
        ability_soul_1 INTEGER,
        ability_soul_2 INTEGER,
        ability_soul_3 INTEGER,
        edited INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        group_id INTEGER NOT NULL,
        category INTEGER NOT NULL,
        PRIMARY KEY (slot, player_id, group_id, category),
        FOREIGN KEY (group_id, player_id, category) REFERENCES players_party_groups (id, player_id, category) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    // database.prepare(`CREATE TABLE IF NOT EXISTS players_party_options (
    //     allow_other_players_to_heal_me INTEGER NOT NULL,
    //     slot INTEGER NOT NULL,
    //     player_id INTEGER NOT NULL,
    //     group_id INTEGER NOT NULL,
    //     category INTEGER NOT NULL,
    //     PRIMARY KEY (slot, player_id, group_id, category),
    //     FOREIGN KEY (slot, player_id, group_id, category) REFERENCES players_parties (slot, player_id, group_id, category) ON DELETE CASCADE,
    //     FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    // )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_equipment (
        id INTEGER NOT NULL,
        level INTEGER NOT NULL,
        enhancement_level INTEGER NOT NULL,
        protection INTEGER NOT NULL,
        stack INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_quest_progress (
        section INTEGER NOT NULL,
        quest_id INTEGER NOT NULL,
        finished INTEGER NOT NULL,
        high_score INTEGER,
        clear_rank INTEGER,
        best_elapsed_time_ms INTEGER,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (section, quest_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_gacha_info (
        gacha_id INTEGER NOT NULL,
        is_daily_first INTEGER NOT NULL,
        is_account_first INTEGER NOT NULL,
        gacha_exchange_point INTEGER,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (gacha_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_gacha_campaigns (
        gacha_id INTEGER NOT NULL,
        campaign_id INTEGER NOT NULL,
        count INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (gacha_id, campaign_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_drawn_quests (
        category_id INTEGER NOT NULL,
        quest_id INTEGER NOT NULL,
        odds_id INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (category_id, quest_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_periodic_reward_points (
        id INTEGER NOT NULL,
        point INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_active_missions (
        id INTEGER NOT NULL,
        progress INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_active_missions_stages (
        id INTEGER NOT NULL,
        status INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        mission_id INTEGER NOT NULL,
        PRIMARY KEY (id, mission_id, player_id),
        FOREIGN KEY (mission_id, player_id) REFERENCES players_active_missions (id, player_id) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS players_box_gacha (
        id INTEGER NOT NULL,
        box_id INTEGER NOT NULL,
        reset_times INTEGER NOT NULL,
        remaining_number INTEGER NOT NULL,
        is_closed INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, box_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_box_gacha_drawn_rewards (
        id INTEGER NOT NULL,
        box_id INTEGER NOT NULL,
        gacha_id INTEGER NOT NULL,
        number INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (id, box_id, gacha_id, player_id),
        FOREIGN KEY (gacha_id, box_id, player_id) REFERENCES players_box_gacha (id, box_id, player_id) ON DELETE CASCADE,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_start_dash_exchange_campaigns (
        campaign_id INTEGER NOT NULL,
        gacha_id INTEGER NOT NULL,
        term_index INTEGER NOT NULL,
        status INTEGER NOT NULL,
        period_start_time DATE NOT NULL,
        period_end_time DATE NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (campaign_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_multi_special_exchange_campaigns (
        campaign_id INTEGER NOT NULL,
        status INTEGER NOT NULL,
        player_id INTEGER NOT NULL,
        PRIMARY KEY (campaign_id, player_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run();

    database.prepare(`CREATE TABLE IF NOT EXISTS players_rush_events (
        player_id INTEGER NOT NULL,
        event_id INTEGER NOT NULL,
        active_rush_battle_folder_id INTEGER,
        endless_battle_max_round INTEGER,
        endless_battle_max_round_time INTEGER,
        endless_battle_max_round_character_id_1 INTEGER,
        endless_battle_max_round_character_id_2 INTEGER,
        endless_battle_max_round_character_id_3 INTEGER,
        endless_battle_max_round_character_evolution_img_lvl_1 INTEGER,
        endless_battle_max_round_character_evolution_img_lvl_2 INTEGER,
        endless_battle_max_round_character_evolution_img_lvl_3 INTEGER,
        PRIMARY KEY (player_id, event_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS players_rush_events_cleared_folders (
        player_id INTEGER NOT NULL,
        event_id INTEGER NOT NULL,
        folder_id INTEGER NOT NULL,
        PRIMARY KEY (player_id, event_id, folder_id),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS players_rush_events_played_parties (
        character_id_1 INTEGER,
        character_id_2 INTEGER,
        character_id_3 INTEGER,
        unison_character_id_1 INTEGER,
        unison_character_id_2 INTEGER,
        unison_character_id_3 INTEGER,
        equipment_id_1 INTEGER,
        equipment_id_2 INTEGER,
        equipment_id_3 INTEGER,
        ability_soul_id_1 INTEGER,
        ability_soul_id_2 INTEGER,
        ability_soul_id_3 INTEGER,
        evolution_img_level_1 INTEGER,
        evolution_img_level_2 INTEGER,
        evolution_img_level_3 INTEGER,
        unison_evolution_img_level_1 INTEGER,
        unison_evolution_img_level_2 INTEGER,
        unison_evolution_img_level_3 INTEGER,
        player_id INTEGER NOT NULL,
        event_id INTEGER NOT NULL,
        round INTEGER NOT NULL,
        battle_type INTEGER NOT NULL,
        PRIMARY KEY (player_id, event_id, round, battle_type),
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()

    database.prepare(`CREATE TABLE IF NOT EXISTS player_mails (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        player_id INTEGER NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL,
        sender TEXT NOT NULL,
        rewards_json TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER,
        received_at INTEGER,
        FOREIGN KEY (player_id) REFERENCES players (id) ON DELETE CASCADE
    )`).run()
}
