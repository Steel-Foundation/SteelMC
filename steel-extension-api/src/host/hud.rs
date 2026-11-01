//! Host side of the per-player HUD API.
use super::{events::parse_text, players::find};
use crate::{
    PlayerId,
    hud::{BarColor, BarStyle, BossBar},
    players::Uuid,
};
use stabby::string::String as ApiString;
use steel_core::{player::hud, server::Server as CoreServer};
use steel_protocol::packets::game::{BossBarColor, BossBarOverlay};

const fn color(color: BarColor) -> BossBarColor {
    match color {
        BarColor::Pink => BossBarColor::Pink,
        BarColor::Blue => BossBarColor::Blue,
        BarColor::Red => BossBarColor::Red,
        BarColor::Green => BossBarColor::Green,
        BarColor::Yellow => BossBarColor::Yellow,
        BarColor::Purple => BossBarColor::Purple,
        BarColor::White => BossBarColor::White,
    }
}

const fn overlay(style: BarStyle) -> BossBarOverlay {
    match style {
        BarStyle::Progress => BossBarOverlay::Progress,
        BarStyle::Notched6 => BossBarOverlay::Notched6,
        BarStyle::Notched10 => BossBarOverlay::Notched10,
        BarStyle::Notched12 => BossBarOverlay::Notched12,
        BarStyle::Notched20 => BossBarOverlay::Notched20,
    }
}

const fn uuid(id: Uuid) -> uuid::Uuid {
    uuid::Uuid::from_bytes(id.bytes)
}

pub(crate) fn set_sidebar(
    server: &CoreServer,
    id: PlayerId,
    title: &str,
    lines: &[ApiString],
) -> Result<(), String> {
    let lines: Vec<_> = lines.iter().map(|line| parse_text(line.as_str())).collect();
    find(server, id)?.set_sidebar(parse_text(title), &lines);
    Ok(())
}

pub(crate) fn clear_sidebar(server: &CoreServer, id: PlayerId) -> Result<(), String> {
    find(server, id)?.clear_sidebar();
    Ok(())
}

pub(crate) fn set_boss_bar(
    server: &CoreServer,
    id: PlayerId,
    bar_id: Uuid,
    bar: &BossBar,
) -> Result<(), String> {
    let flags =
        u8::from(bar.darken_screen) | u8::from(bar.boss_music) << 1 | u8::from(bar.fog) << 2;
    find(server, id)?.set_boss_bar(
        uuid(bar_id),
        hud::BossBar {
            title: parse_text(bar.title.as_str()),
            progress: bar.progress,
            color: color(bar.color),
            overlay: overlay(bar.style),
            flags,
        },
    );
    Ok(())
}

pub(crate) fn remove_boss_bar(
    server: &CoreServer,
    id: PlayerId,
    bar_id: Uuid,
) -> Result<(), String> {
    find(server, id)?.remove_boss_bar(uuid(bar_id));
    Ok(())
}

pub(crate) fn set_tab_list(
    server: &CoreServer,
    id: PlayerId,
    header: &str,
    footer: &str,
) -> Result<(), String> {
    find(server, id)?.set_tab_list_override(parse_text(header), parse_text(footer));
    Ok(())
}

pub(crate) fn clear_tab_list(server: &CoreServer, id: PlayerId) -> Result<(), String> {
    find(server, id)?.clear_tab_list_override();
    Ok(())
}
