mod focus;
mod typist;

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{AppHandle, State};
use typing_plan::{Plan, PlanConfig};

use focus::Target;
use typist::{Control, Options};

#[derive(Default)]
struct Typer {
    control: Mutex<Option<Sender<Control>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRequest {
    /// The text to type: the whole text, or just the selected part.
    text: String,
    config: PlanConfig,
    pause_on_click: bool,
    /// Where it typed last time, to go straight back there.
    remembered: Option<Target>,
}

impl Typer {
    fn send(&self, msg: Control) -> Result<(), String> {
        let control = self.control.lock().unwrap();
        let tx = control.as_ref().ok_or("Not typing")?;
        tx.send(msg).map_err(|_| "Typing already finished".to_string())
    }
}

#[tauri::command]
fn start_typing(app: AppHandle, typer: State<Typer>, request: StartRequest) -> Result<(), String> {
    if request.text.is_empty() {
        return Err("Nothing to type".into());
    }
    let mut control = typer.control.lock().unwrap();
    if let Some(tx) = control.take() {
        let _ = tx.send(Control::Stop);
    }
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let plan = Plan::new(&request.text, request.config, seed);
    let (tx, rx) = mpsc::channel();
    let opts = Options { pause_on_click: request.pause_on_click, remembered: request.remembered };
    thread::Builder::new()
        .name("typist".into())
        .spawn(move || typist::run(app, plan, rx, opts))
        .map_err(|e| e.to_string())?;
    *control = Some(tx);
    Ok(())
}

#[tauri::command]
fn pause_typing(typer: State<Typer>) -> Result<(), String> {
    typer.send(Control::Pause)
}

#[tauri::command]
fn resume_typing(typer: State<Typer>) -> Result<(), String> {
    typer.send(Control::Resume)
}

#[tauri::command]
fn stop_typing(typer: State<Typer>) -> Result<(), String> {
    typer.send(Control::Stop)
}

/// Speed and typo changes apply from the next key, even mid-run.
#[tauri::command]
fn update_typing(typer: State<Typer>, wpm: u32, typo_rate: f64) {
    let _ = typer.send(Control::SetWpm(wpm));
    let _ = typer.send(Control::SetTypoRate(typo_rate));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Typer::default())
        .invoke_handler(tauri::generate_handler![
            start_typing,
            pause_typing,
            resume_typing,
            stop_typing,
            update_typing
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
