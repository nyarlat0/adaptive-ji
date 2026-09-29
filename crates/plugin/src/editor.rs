use super::*;
use adaptive_ji_core::settings::EmbeddedScale;
use adaptive_ji_core::{
    settings::AttackMode,
    voice::{VoiceState, note_name},
};
use logic_nih_plug_egui::{create_egui_editor, egui};

struct Draft {
    value: serde_json::Value,
    scl: String,
    kbm: String,
    spectrum: String,
    message: String,
    live: LiveSnapshot,
}

fn live_notes(ui: &mut egui::Ui, snapshot: &LiveSnapshot) {
    ui.heading("Live notes / Звучащие ноты");
    let sounding = snapshot
        .voices
        .iter()
        .flatten()
        .filter(|v| !v.pending)
        .count();
    let waiting = snapshot
        .voices
        .iter()
        .flatten()
        .filter(|v| v.pending)
        .count();
    ui.label(format!(
        "Звучат: {sounding}   Ожидают расчёта: {waiting}{}",
        if snapshot.optimizing {
            "   ·   Подбирается строй…"
        } else {
            ""
        }
    ));
    if let Some((id, note)) = snapshot.reference {
        ui.label(format!(
            "Отношения к {} (voice {id}) — опора последнего решения, не обязательно тоника.",
            note_name(note)
        ));
    }
    ui.label("Текущая / целевая поправка: центы к 12-TET входной ноты, включая Scala.");
    egui::ScrollArea::both()
        .id_salt("live-notes")
        .max_height(400.0)
        .show(ui, |ui| {
            egui::Grid::new("voices")
                .striped(true)
                .spacing([20.0, 8.0])
                .show(ui, |ui| {
                    for title in [
                        "Нота · ID",
                        "Состояние",
                        "Отношение",
                        "Сейчас → цель",
                        "Выход",
                    ] {
                        ui.label(egui::RichText::new(title).size(19.0).strong());
                    }
                    ui.end_row();
                    for voice in snapshot.voices.iter().flatten() {
                        let state = if voice.pending {
                            "Ожидает JI"
                        } else if voice.state == VoiceState::ReleasedSustained {
                            "Педаль"
                        } else {
                            "Звучит"
                        };
                        let ratio = voice
                            .target
                            .map_or_else(|| "—".into(), |t| t.ratio.to_string());
                        let current = voice
                            .current_cents
                            .map_or_else(|| "—".into(), |c| format!("{c:+.2}"));
                        let target = voice
                            .target
                            .map_or_else(|| "—".into(), |t| format!("{:+.2}c", t.cents_offset));
                        for text in [
                            format!("{} · {}", note_name(voice.input_note), voice.id),
                            state.into(),
                            ratio,
                            format!("{current} → {target}"),
                            format!(
                                "{} ch{}",
                                note_name(voice.output_note),
                                voice.output_channel + 1
                            ),
                        ] {
                            ui.label(egui::RichText::new(text).monospace().size(22.0));
                        }
                        ui.end_row();
                    }
                });
        });
    if sounding + waiting == 0 {
        ui.label(egui::RichText::new("Нет активных нот").size(22.0));
    }
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
        live: LiveSnapshot::default(),
    };
    create_egui_editor(
        params.editor.clone(),
        draft,
        |ctx, _| {
            let mut style = (*ctx.style()).clone();
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(18.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(18.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
            ctx.set_style(style);
        },
        move |ctx, setter, draft| {
            while let Some(snapshot) = control.live.pop() {
                draft.live = snapshot;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
            egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("adaptive-ji — MPE note effect");
            ui.label(match draft.live.attack_mode { AttackMode::Immediate => "Активный режим: немедленная атака в базовом строе", AttackMode::Tuned => "Активный режим: атака после подбора JI (переменная задержка)" });
            ui.horizontal_wrapped(|ui| {
                if ui.button("PANIC").clicked() { control.panic.store(true,Ordering::Release); }
                if ui.button("Reload current settings").clicked() { draft.value = serde_json::to_value(&*params.settings.lock().unwrap()).unwrap(); }
                let apply = egui::Button::new(egui::RichText::new("Validate and apply").color(egui::Color32::WHITE).strong())
                    .fill(egui::Color32::from_rgb(32, 120, 64));
                if ui.add_enabled(!control.busy.load(Ordering::Acquire), apply).clicked() {
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
                if ui.button("Reset to defaults").on_hover_text("Сбросить настройки редактора, Scala и custom spectrum. Для применения нажмите Validate and apply.").clicked() {
                    draft.value = serde_json::to_value(EngineSettings::default()).unwrap();
                    draft.scl.clear();
                    draft.kbm.clear();
                    draft.spectrum.clear();
                    draft.message = "Дефолтные настройки загружены в редактор. Нажмите Validate and apply для применения.".into();
                }
            });
            ui.label(&draft.message);
            ui.label(control.message.lock().unwrap().as_str());
            egui::ScrollArea::vertical().show(ui, |ui| {
                live_notes(ui, &draft.live);
                ui.separator();
                ui.heading("Note attack / Атака ноты");
                let mut attack = draft.value["attack_mode"].as_str().unwrap_or("immediate").to_owned();
                egui::ComboBox::from_id_salt("attack-mode").width(500.0)
                    .selected_text(if attack == "tuned" { "Сначала подобрать строй (ждать JI)" } else { "Сразу: базовый строй → JI через glide" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut attack, "immediate".into(), "Сразу: базовый строй → JI через glide");
                        ui.selectable_value(&mut attack, "tuned".into(), "Сначала подобрать строй (ждать JI)");
                    });
                draft.value["attack_mode"] = attack.into();
                ui.label("Ждать JI: target Bend → NoteOn, без glide при атаке. Задержка зависит от расчёта.");
                ui.label("Короткая нота, отпущенная до расчёта без педали, не прозвучит.");
                ui.label("Выберите режим и нажмите зелёную Validate and apply сверху. Apply снимает текущие ноты.");
                if let Some(map) = draft.value.as_object_mut() {
                    for (key,value) in map.iter_mut().filter(|(k,_)| !matches!(k.as_str(),"scale"|"custom_spectrum_toml"|"attack_mode")) {
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
                ui.label("Set the receiving synth to MPE and the same bend range. No audio is generated.");
            });
        });
        },
    )
}
