//! Save games: the whole world (plant, career, grid station, time settings)
//! as JSON files in `./saves` (or `$NR_SAVE_DIR`).

use crate::world::World;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Bumped when the save format changes incompatibly.
const SAVE_VERSION: u32 = 1;
/// Name of the automatic save slot.
pub const AUTOSAVE: &str = "autosave";

#[derive(Serialize, Deserialize)]
struct SaveGame {
    version: u32,
    /// Seconds since the Unix epoch.
    saved_at: u64,
    plant: nr_reactors::Pwr,
    career: Option<nr_game::Career>,
    station: nr_game::Station,
    time_scale: f64,
    #[serde(default)]
    auto_warp: bool,
    #[serde(default)]
    warp_factor: f64,
    #[serde(default)]
    ramp_factor: f64,
}

/// Summary of a save file for the load list.
#[derive(Serialize)]
pub struct SaveInfo {
    pub name: String,
    pub saved_at: u64,
    pub sim_time: f64,
    pub mode: String,
    pub scenario: String,
    pub unit: String,
    pub career_date: Option<String>,
    pub money: Option<f64>,
    pub bytes: u64,
}

fn dir() -> PathBuf {
    std::env::var_os("NR_SAVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("saves"))
}

/// Save names: letters, digits, space, dash, underscore; 1–40 chars.
fn check_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() || n.len() > 40 {
        return Err("save name must be 1-40 characters".into());
    }
    if !n
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        return Err("save name may only use letters, digits, spaces, - and _".into());
    }
    Ok(n.to_string())
}

fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn save(w: &World, name: &str) -> Result<(), String> {
    let name = check_name(name)?;
    let game = SaveGame {
        version: SAVE_VERSION,
        saved_at: now(),
        plant: w.plant.clone(),
        career: w.career.clone(),
        station: w.station.clone(),
        time_scale: w.time_scale,
        auto_warp: w.auto_warp,
        warp_factor: w.warp_factor,
        ramp_factor: w.ramp_factor,
    };
    let json = serde_json::to_string(&game).map_err(|e| format!("cannot encode save: {e}"))?;
    std::fs::create_dir_all(dir()).map_err(|e| format!("cannot create save folder: {e}"))?;
    // Write to a temporary file first so a crash can't leave half a save.
    let tmp = dir().join(format!(".{name}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| format!("cannot write save: {e}"))?;
    std::fs::rename(&tmp, path(&name)).map_err(|e| format!("cannot write save: {e}"))?;
    Ok(())
}

pub fn load(w: &mut World, name: &str) -> Result<(), String> {
    let name = check_name(name)?;
    let text =
        std::fs::read_to_string(path(&name)).map_err(|_| format!("no save named '{name}'"))?;
    let game: SaveGame =
        serde_json::from_str(&text).map_err(|e| format!("save '{name}' is unreadable: {e}"))?;
    if game.version != SAVE_VERSION {
        return Err(format!(
            "save '{name}' is from an incompatible version ({})",
            game.version
        ));
    }
    w.plant = game.plant.restored();
    w.career = game.career.map(|mut c| {
        c.upgrades.retain(|u| nr_api::upgrades::in_catalog(*u));
        c
    });
    w.station = game.station;
    w.time_scale = game.time_scale;
    w.auto_warp = game.auto_warp;
    if game.warp_factor > 0.0 {
        w.warp_factor = game.warp_factor;
    }
    if game.ramp_factor > 0.0 {
        w.ramp_factor = game.ramp_factor;
    }
    w.warping = false;
    w.paused = false;
    Ok(())
}

pub fn delete(name: &str) -> Result<(), String> {
    let name = check_name(name)?;
    std::fs::remove_file(path(&name)).map_err(|_| format!("no save named '{name}'"))
}

pub fn list() -> Vec<SaveInfo> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out: Vec<SaveInfo> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "json" {
                return None;
            }
            let name = p.file_stem()?.to_str()?.to_string();
            check_name(&name).ok()?;
            let bytes = e.metadata().ok()?.len();
            let text = std::fs::read_to_string(&p).ok()?;
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let career = v.get("career").filter(|c| !c.is_null());
            Some(SaveInfo {
                name,
                saved_at: v.get("saved_at")?.as_u64().unwrap_or(0),
                sim_time: v.pointer("/plant/time")?.as_f64().unwrap_or(0.0),
                mode: if career.is_some() {
                    "career"
                } else {
                    "sandbox"
                }
                .into(),
                scenario: v
                    .pointer("/plant/scenario")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .into(),
                unit: v
                    .pointer("/plant/p/gross_mwe")
                    .and_then(|g| g.as_f64())
                    .map(|g| format!("{g:.0} MWe"))
                    .unwrap_or_default(),
                career_date: career.and_then(|c| {
                    let year = c.get("start_year")?.as_i64()?;
                    let days = c.get("elapsed_days")?.as_f64()?;
                    Some(format!("{year} + {:.0} days", days))
                }),
                money: career.and_then(|c| c.get("money")?.as_f64()),
                bytes,
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.saved_at));
    out
}
