use super::*;
use adaptive_ji_core::settings::EmbeddedScale;
use logic_nih_plug_egui::{create_egui_editor, egui};

struct Draft {
    value: serde_json::Value,
    scl: String,
    kbm: String,
    spectrum: String,
    message: String,
}
fn fields(ui: &mut egui::Ui, value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if value.is_object() {
                    ui.collapsing(key, |ui| fields(ui, value));
                } else {
                    ui.horizontal(|ui| {
                        ui.label(key);
                        fields(ui, value);
                    });
                }
            }
        }
        serde_json::Value::Bool(value) => {
            ui.checkbox(value, "");
        }
        serde_json::Value::Number(value) => {
            if let Some(mut number) = value.as_u64() {
                if ui.add(egui::DragValue::new(&mut number)).changed() {
                    *value = number.into();
                }
            } else if let Some(mut number) = value.as_f64()
                && ui
                    .add(egui::DragValue::new(&mut number).speed(0.05))
                    .changed()
                && let Some(number) = serde_json::Number::from_f64(number)
            {
                *value = number;
            }
        }
        serde_json::Value::String(value) => {
            ui.text_edit_singleline(value);
        }
        _ => {
            ui.label("Use the file controls below");
        }
    }
}
pub(super) fn create(
    params: Arc<Parameters>,
    control: Arc<Control>,
    executor: AsyncExecutor<AdaptiveJiPlugin>,
) -> Option<Box<dyn Editor>> {
    let draft = Draft {
        value: serde_json::to_value(&*params.settings.lock().unwrap()).unwrap(),
        scl: String::new(),
        kbm: String::new(),
        spectrum: String::new(),
        message: String::new(),
    };
    create_egui_editor(
        params.editor.clone(),
        draft,
        |_, _| {},
        move |ctx, setter, draft| {
            egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("adaptive-ji — MPE note effect");
            ui.label("Immediate notes · asynchronous adaptive tuning · zero declared latency");
            ui.horizontal(|ui| {
                if ui.button("PANIC").clicked() { control.panic.store(true,Ordering::Release); }
                if ui.button("Reload current settings").clicked() { draft.value = serde_json::to_value(&*params.settings.lock().unwrap()).unwrap(); }
            });
            ui.label("Configuration is non-automatable. Apply releases current notes.");
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(map) = draft.value.as_object_mut() {
                    for (key,value) in map.iter_mut().filter(|(k,_)| !matches!(k.as_str(),"scale"|"custom_spectrum_toml")) {
                        ui.collapsing(key, |ui| fields(ui,value));
                    }
                }
                ui.separator();
                ui.label("Scala: filenames are used only for import; contents are embedded in project state.");
                ui.horizontal(|ui| {ui.label(".scl path"); ui.text_edit_singleline(&mut draft.scl);});
                ui.horizontal(|ui| {ui.label(".kbm path (optional)"); ui.text_edit_singleline(&mut draft.kbm);});
                if ui.button("Import Scala into draft").clicked() {
                    let load = || -> Result<EmbeddedScale,String> { Ok(EmbeddedScale {
                        name: std::path::Path::new(&draft.scl).file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        scl:std::fs::read_to_string(&draft.scl).map_err(|e|e.to_string())?,
                        kbm:if draft.kbm.trim().is_empty() {None} else {Some(std::fs::read_to_string(&draft.kbm).map_err(|e|e.to_string())?)}, root_note:60,
                    }) };
                    match load() { Ok(scale) => draft.value["scale"] = serde_json::to_value(scale).unwrap(), Err(error) => draft.message=error }
                }
                if !draft.value["scale"].is_null() {
                    ui.label(format!("Draft scale: {}", draft.value["scale"]["name"]));
                    ui.horizontal(|ui| {ui.label("root_note (without KBM)"); fields(ui, &mut draft.value["scale"]["root_note"]);});
                }
                if ui.button("Clear scale / 12-TET base").clicked() {draft.value["scale"]=serde_json::Value::Null;}
                ui.horizontal(|ui| {ui.label("Custom spectrum TOML"); ui.text_edit_singleline(&mut draft.spectrum);});
                if ui.button("Import spectrum into draft").clicked() {
                    match std::fs::read_to_string(&draft.spectrum) {Ok(text)=>draft.value["custom_spectrum_toml"]=text.into(),Err(e)=>draft.message=e.to_string()}
                }
                if ui.button("Use built-in spectrum").clicked() {draft.value["custom_spectrum_toml"]=serde_json::Value::Null;}
                if ui.add_enabled(!control.busy.load(Ordering::Acquire), egui::Button::new("Validate and apply")).clicked() {
                    match serde_json::from_value::<EngineSettings>(draft.value.clone()).and_then(|settings| settings.prepare().map(|_|settings).map_err(serde::de::Error::custom)) {
                        Ok(settings) => {
                            control.busy.store(true,Ordering::Release);
                            executor.execute_background(settings);
                            setter.begin_set_parameter(&params.revision);
                            setter.set_parameter(&params.revision, (params.revision.value() + 1) % 1024);
                            setter.end_set_parameter(&params.revision);
                            draft.message.clear();
                        }
                        Err(error) => draft.message = error.to_string(),
                    }
                }
                ui.label(&draft.message);
                ui.label(control.message.lock().unwrap().as_str());
                ui.label("Set the receiving synth to MPE and the same bend range. No audio is generated.");
            });
        });
        },
    )
}
