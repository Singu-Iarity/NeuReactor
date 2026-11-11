//! HTTP routing for the UI and the `/api/v1` JSON API.

use crate::world::{random_token, PluginRecord, World};
use nr_api::{can_read, modules, Command, Origin, Scope, API_VERSION};
use nr_reactors::PlantEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Mutex;
use tiny_http::{Header, Method, Request, Response};

include!(concat!(env!("OUT_DIR"), "/web_files.rs"));

fn web_file(path: &str) -> Option<(&'static str, &'static [u8])> {
    let path = if path == "/" { "/index.html" } else { path };
    WEB_FILES.iter().find(|f| f.0 == path).map(|f| (f.1, f.2))
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn respond(req: Request, status: u16, content_type: &str, body: String) {
    let resp = Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header(
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type",
        ))
        .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
    let _ = req.respond(resp);
}

fn json_resp(req: Request, status: u16, v: Value) {
    respond(req, status, "application/json", v.to_string());
}

fn error(req: Request, status: u16, msg: &str) {
    json_resp(req, status, json!({ "ok": false, "error": msg }));
}

fn bearer(req: &Request) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer ").map(str::to_string))
        .or_else(|| {
            let url = req.url();
            let q = url.split_once('?')?.1;
            q.split('&')
                .find_map(|kv| kv.strip_prefix("token=").map(str::to_string))
        })
}

fn query_u64(url: &str, key: &str) -> Option<u64> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        kv.strip_prefix(&format!("{key}="))
            .and_then(|v| v.parse().ok())
    })
}

#[derive(Deserialize)]
struct RegisterReq {
    name: String,
    scopes: Vec<Scope>,
}

#[derive(Deserialize)]
struct ApproveReq {
    id: String,
    approve: bool,
}

pub fn handle(mut req: Request, world: &Mutex<World>) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();

    if method == Method::Options {
        return respond(req, 204, "text/plain", String::new());
    }

    let mut body = String::new();
    if method == Method::Post
        && req
            .as_reader()
            .take(64 * 1024)
            .read_to_string(&mut body)
            .is_err()
    {
        return error(req, 400, "body must be UTF-8 JSON");
    }

    match (&method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let token = world.lock().unwrap().operator_token.clone();
            let html = String::from_utf8_lossy(web_file("/").map(|f| f.1).unwrap_or_default());
            respond(
                req,
                200,
                "text/html; charset=utf-8",
                html.replace("__OPERATOR_TOKEN__", &token),
            )
        }
        (Method::Get, p) if !p.starts_with("/api/") && web_file(p).is_some() => {
            let (mime, bytes) = web_file(p).unwrap();
            let resp = Response::from_data(bytes.to_vec())
                .with_header(header("Content-Type", mime))
                .with_header(header("Cache-Control", "no-store"));
            let _ = req.respond(resp);
        }

        (Method::Get, "/api/v1") => json_resp(
            req,
            200,
            json!({
                "name": "NeuReactor",
                "api_version": API_VERSION,
                "version": env!("CARGO_PKG_VERSION"),
                "scopes": ["telemetry_read","truth_read","control_write","sim_control","scenario_write","economy_read","ui_panel"],
                "modules": modules::CATALOG,
                "endpoints": {
                    "GET /api/v1/telemetry": "instrument readings, alarms, modules (telemetry_read)",
                    "GET /api/v1/truth": "true physics state (truth_read, sandbox only)",
                    "GET /api/v1/events?since=SEQ": "event log (telemetry_read)",
                    "POST /api/v1/command": "issue a Command (see docs/api/README.md)",
                    "POST /api/v1/plugins/register": "{name, scopes} -> {id, token}; operator must approve",
                    "GET /api/v1/plugins/me": "registration status of the calling token",
                }
            }),
        ),

        (Method::Post, "/api/v1/plugins/register") => {
            let Ok(r) = serde_json::from_str::<RegisterReq>(&body) else {
                return error(req, 400, "expected {\"name\": string, \"scopes\": [scope]}");
            };
            let mut w = world.lock().unwrap();
            if w.plugins.values().filter(|p| !p.approved).count() >= 16 {
                drop(w);
                return error(req, 429, "too many pending plugin registrations");
            }
            let id = random_token()[..12].to_string();
            let token = random_token();
            let name: String = r.name.chars().take(64).collect();
            w.plugins.insert(
                id.clone(),
                PluginRecord {
                    id: id.clone(),
                    name: name.clone(),
                    scopes: r.scopes.into_iter().collect(),
                    approved: false,
                    token: token.clone(),
                },
            );
            w.log(PlantEvent::info(&format!(
                "Plugin '{name}' requests access"
            )));
            drop(w);
            json_resp(
                req,
                200,
                json!({ "ok": true, "id": id, "token": token, "status": "pending" }),
            )
        }

        (Method::Get, "/api/v1/plugins/me") => {
            let tok = bearer(&req);
            let w = world.lock().unwrap();
            let rec = w
                .plugins
                .values()
                .find(|p| Some(&p.token) == tok.as_ref())
                .cloned();
            drop(w);
            match rec {
                Some(p) => json_resp(req, 200, json!({ "ok": true, "plugin": p })),
                None => error(req, 404, "unknown token"),
            }
        }

        _ => api_authed(req, world, &method, &path, &url, &body),
    }
}

fn api_authed(
    req: Request,
    world: &Mutex<World>,
    method: &Method,
    path: &str,
    url: &str,
    body: &str,
) {
    let tok = bearer(&req);
    let mut w = world.lock().unwrap();
    let Some(origin) = w.origin(tok.as_deref()) else {
        drop(w);
        return error(req, 401, "missing or unapproved token");
    };
    let mode = w.plant.game_mode();
    let is_operator = origin == Origin::Operator;

    match (method, path) {
        (Method::Get, "/api/v1/telemetry") => {
            if !can_read(&origin, Scope::TelemetryRead, mode) {
                drop(w);
                return error(req, 403, "requires telemetry_read");
            }
            let mut t = w.plant.telemetry(w.time_scale, w.paused);
            t.alarms.extend(w.station.alarms(&w.plant));
            drop(w);
            json_resp(req, 200, serde_json::to_value(t).unwrap())
        }
        (Method::Get, "/api/v1/truth") => {
            if !can_read(&origin, Scope::TruthRead, mode) {
                drop(w);
                return error(req, 403, "requires truth_read (sandbox only)");
            }
            let t = w.plant.truth();
            drop(w);
            json_resp(req, 200, json!(t))
        }
        (Method::Get, "/api/v1/events") => {
            if !can_read(&origin, Scope::TelemetryRead, mode) {
                drop(w);
                return error(req, 403, "requires telemetry_read");
            }
            let since = query_u64(url, "since").unwrap_or(0);
            let ev: Vec<_> = w.events.iter().filter(|e| e.seq > since).cloned().collect();
            drop(w);
            json_resp(req, 200, json!(ev))
        }
        (Method::Post, "/api/v1/command") => {
            let cmd: Command = match serde_json::from_str(body) {
                Ok(c) => c,
                Err(e) => {
                    drop(w);
                    return error(req, 400, &format!("bad command: {e}"));
                }
            };
            let result = w.plant.command(&cmd, &origin);
            if result.is_ok() {
                match &cmd {
                    Command::SetTimeScale { factor } => w.time_scale = factor.clamp(0.1, 10_000.0),
                    Command::SetPaused { paused } => w.paused = *paused,
                    _ => {}
                }
            }
            w.collect_events();
            drop(w);
            match result {
                Ok(()) => json_resp(req, 200, json!({ "ok": true })),
                Err(e) => error(req, 403, &e.0),
            }
        }
        (Method::Get, "/api/v1/career") if is_operator => {
            let v = match &w.career {
                Some(c) => json!({ "active": true, "career": c.view() }),
                None => json!({ "active": false }),
            };
            drop(w);
            json_resp(req, 200, v)
        }
        (Method::Post, p) if is_operator && p.starts_with("/api/v1/career/") => {
            let action = &p["/api/v1/career/".len()..];
            let result = career_action(&mut w, action, body);
            w.collect_events();
            drop(w);
            match result {
                Ok(()) => json_resp(req, 200, json!({ "ok": true })),
                Err(e) => error(req, 400, &e),
            }
        }
        (Method::Get, "/api/v1/station") if is_operator => {
            let st = &w.station;
            let v = json!({
                "demand_mwe": st.demand_mwe,
                "pending": st.pending,
                "deadline": st.deadline,
                "agc": st.agc,
                "on_target_s": st.on_target_s,
                "off_target_s": st.off_target_s,
                "boron_sample": st.boron_sample,
                "messages": st.messages.iter().rev().take(80).collect::<Vec<_>>(),
                "unacked": st.unacked(),
                "sim_time": w.plant.time,
            });
            drop(w);
            json_resp(req, 200, v)
        }
        (Method::Post, p) if is_operator && p.starts_with("/api/v1/station/") => {
            let action = &p["/api/v1/station/".len()..];
            let result = station_action(&mut w, action, body);
            w.collect_events();
            drop(w);
            match result {
                Ok(()) => json_resp(req, 200, json!({ "ok": true })),
                Err(e) => error(req, 400, &e),
            }
        }
        (Method::Get, "/api/v1/plugins") if is_operator => {
            let list: Vec<_> = w.plugins.values().cloned().collect();
            drop(w);
            json_resp(req, 200, json!(list))
        }
        (Method::Post, "/api/v1/plugins/approve") if is_operator => {
            let Ok(a) = serde_json::from_str::<ApproveReq>(body) else {
                drop(w);
                return error(req, 400, "expected {\"id\": string, \"approve\": bool}");
            };
            let msg = if a.approve {
                w.plugins.get_mut(&a.id).map(|p| {
                    p.approved = true;
                    format!("Plugin '{}' approved with scopes {:?}", p.name, p.scopes)
                })
            } else {
                w.plugins
                    .remove(&a.id)
                    .map(|p| format!("Plugin '{}' revoked", p.name))
            };
            match msg {
                Some(m) => {
                    w.log(PlantEvent::info(&m));
                    drop(w);
                    json_resp(req, 200, json!({ "ok": true }))
                }
                None => {
                    drop(w);
                    error(req, 404, "unknown plugin id")
                }
            }
        }
        _ => {
            drop(w);
            error(req, 404, "not found")
        }
    }
}

#[derive(Deserialize)]
struct NewCareer {
    start_year: i32,
    difficulty: nr_game::Difficulty,
}

fn career_action(w: &mut World, action: &str, body: &str) -> Result<(), String> {
    let parse = |e: serde_json::Error| format!("bad request: {e}");
    match action {
        "new" => {
            let n: NewCareer = serde_json::from_str(body).map_err(parse)?;
            if !(1965..=2030).contains(&n.start_year) {
                return Err("start year must be 1965-2030".into());
            }
            w.career = Some(nr_game::Career::new(n.start_year, n.difficulty));
            // The site is under construction: hold the simulation.
            w.plant = nr_reactors::Pwr::hot_standby();
            w.station = nr_game::Station::new();
            w.plant.automation.game_mode = nr_api::GameMode::Career;
            w.paused = true;
            Ok(())
        }
        "quit" => {
            w.career = None;
            w.plant = nr_reactors::Pwr::hot_standby();
            w.station = nr_game::Station::new();
            w.paused = false;
            Ok(())
        }
        "buy" => {
            let p: nr_game::Purchase = serde_json::from_str(body).map_err(parse)?;
            let World { plant, career, .. } = w;
            career.as_mut().ok_or("no career active")?.buy(p, plant)
        }
        "diesel_fuel" => {
            let World { plant, career, .. } = w;
            career
                .as_mut()
                .ok_or("no career active")?
                .buy_diesel_fuel(plant)
        }
        "research" => {
            let u: nr_api::Unlock = serde_json::from_str(body).map_err(parse)?;
            w.career
                .as_mut()
                .ok_or("no career active")?
                .start_research(u)
        }
        "commission" => {
            let plant = w.career.as_mut().ok_or("no career active")?.commission()?;
            w.plant = plant;
            w.station = nr_game::Station::new();
            w.paused = false;
            w.time_scale = 1.0;
            Ok(())
        }
        _ => Err("unknown career action".into()),
    }
}

#[derive(Deserialize)]
struct CallReq {
    call: nr_game::Call,
}

#[derive(Deserialize)]
struct AgcReq {
    on: bool,
}

fn station_action(w: &mut World, action: &str, body: &str) -> Result<(), String> {
    let parse = |e: serde_json::Error| format!("bad request: {e}");
    let t = w.plant.time;
    match action {
        "call" => {
            let c: CallReq = serde_json::from_str(body).map_err(parse)?;
            w.station.call(c.call, t);
            Ok(())
        }
        "ack" => {
            #[derive(Deserialize)]
            struct AckReq {
                seq: Option<u64>,
            }
            let a: AckReq = serde_json::from_str(body).map_err(parse)?;
            w.station.ack(a.seq);
            Ok(())
        }
        "message" => {
            #[derive(Deserialize)]
            struct MsgReq {
                text: String,
            }
            let m: MsgReq = serde_json::from_str(body).map_err(parse)?;
            if m.text.len() > 300 {
                return Err("message too long".into());
            }
            if w.station.typed(&m.text, t) == Some(nr_game::station::Intent::AcceptLoad) {
                return station_action(w, "accept", "{}");
            }
            Ok(())
        }
        "accept" => {
            let target = w.station.accept(t).ok_or("no pending load request")?;
            // Hand the new load to the unit load controller if it is in AUTO,
            // otherwise straight to the turbine governor.
            let gross = target + w.plant.house_load();
            let module = if w.plant.automation.mode(modules::ModuleId::PlantMaster)
                == modules::ModuleMode::Auto
            {
                modules::ModuleId::PlantMaster
            } else {
                modules::ModuleId::TurbineGovernor
            };
            let _ = w.plant.command(
                &Command::SetSetpoint {
                    module,
                    key: "target_mwe".into(),
                    value: gross,
                },
                &Origin::Operator,
            );
            Ok(())
        }
        "agc" => {
            let a: AgcReq = serde_json::from_str(body).map_err(parse)?;
            if a.on && !w.plant.agc_available() {
                return Err("AGC needs the Plant Master / Unit Load Controller in AUTO and the Grid Dispatch Link".into());
            }
            w.station.agc = a.on;
            Ok(())
        }
        _ => Err("unknown station action".into()),
    }
}
