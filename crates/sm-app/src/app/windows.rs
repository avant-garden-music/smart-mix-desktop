use super::*;

impl SmartMixApp {
    pub(crate) fn windows(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        // Rueda Camelot (F-21): panel flotante, filtro real por armonía
        // confirmacion de Quitar de la biblioteca (menu contextual)
        if self.confirm_remove.is_some() {
            let ids = self.confirm_remove.clone().unwrap_or_default();
            let n = ids.len();
            let mut open = true;
            egui::Window::new("Quitar de la biblioteca")
                .open(&mut open)
                .min_width(320.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Se quitaran {n} pista(s) de la biblioteca. Los ARCHIVOS no se borran."
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("Quitar").color(t.magenta))
                            .clicked()
                        {
                            if let Ok(db) = self.db.lock() {
                                for id in &ids {
                                    let _ = db.delete_track(*id);
                                }
                            }
                            eprintln!("[feature] {n} pista(s) quitadas de la biblioteca");
                            self.confirm_remove = None;
                            self.selected_set.clear();
                            self.refresh();
                        }
                        if ui.button("Cancelar").clicked() {
                            self.confirm_remove = None;
                        }
                    });
                });
            if !open {
                self.confirm_remove = None;
            }
        }

        if self.show_prefs {
            let mut open = true;
            egui::Window::new(texts::t(&self.prefs.language, "prefs.title"))
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::ComboBox::from_id_source("lang")
                        .selected_text(self.prefs.language.clone())
                        .show_ui(ui, |ui| {
                            for l in sm_i18n::I18n::available() {
                                if ui
                                    .selectable_label(self.prefs.language == l, l.to_uppercase())
                                    .clicked()
                                {
                                    self.prefs.language = l.to_string();
                                }
                            }
                        });
                    ui.add_space(4.0);
                    ui.add(
                        egui::Slider::new(&mut self.prefs.analysis_workers, 1..=16)
                            .text("hilos de análisis"),
                    );
                    ui.checkbox(&mut self.prefs.analyze_on_import, "Analizar al importar");
                    ui.checkbox(
                        &mut self.prefs.write_tags_id3,
                        "Permitir escritura de tags (requiere confirmación)",
                    );
                    ui.text_edit_singleline(&mut self.prefs.id3_comment_format);
                    if ui.button("Guardar").clicked() {
                        if let Ok(db) = self.db.lock() {
                            let _ = db.set_preferences(&self.prefs);
                        }
                        self.show_prefs = false;
                    }
                });
            self.show_prefs = open;
        }
        if self.show_new_playlist {
            let mut open = true;
            // F-18: título según cree en raíz o dentro de una carpeta
            let title_key = if self.new_playlist_parent.is_some() {
                "playlist.sub"
            } else {
                "playlist.new"
            };
            let parent_name = self.new_playlist_parent.and_then(|pid| {
                self.playlists
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| p.name.clone())
            });
            egui::Window::new(texts::t(&self.prefs.language, title_key))
                .open(&mut open)
                .show(ctx, |ui| {
                    if let Some(pn) = &parent_name {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {pn}",
                                texts::t(&self.prefs.language, "playlist.sub_hint")
                            ))
                            .small()
                            .color(Theme::default().text_dim),
                        );
                    }
                    ui.text_edit_singleline(&mut self.new_playlist_name);
                    // F-18: inserción rápida de emoji (prefijo visual del nombre)
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(texts::t(&self.prefs.language, "playlist.emoji"))
                                .small()
                                .color(Theme::default().text_dim),
                        );
                        for e in [
                            "\u{1F525}",
                            "\u{1F3B6}",
                            "\u{1F311}",
                            "\u{26A1}",
                            "\u{1F30A}",
                            "\u{1F48E}",
                        ] {
                            if ui.small_button(e).clicked() {
                                self.new_playlist_name.push_str(e);
                            }
                        }
                    });
                    ui.checkbox(
                        &mut self.new_playlist_smart,
                        "Playlist inteligente (vista actual como filtro)",
                    );
                    if ui.button("Crear").clicked() && !self.new_playlist_name.is_empty() {
                        let filter = self.new_playlist_smart.then(|| sm_model::SmartFilter {
                            text: if self.search.is_empty() {
                                None
                            } else {
                                Some(self.search.clone())
                            },
                            key_compatible_with: self.compat_key.clone(),
                            ..Default::default()
                        });
                        let kind = if self.new_playlist_smart {
                            "smart"
                        } else {
                            "manual"
                        };
                        let parent = self.new_playlist_parent;
                        if let Ok(mut db) = self.db.lock() {
                            if let Ok(pl) =
                                db.create_playlist(&self.new_playlist_name, kind, filter.as_ref())
                            {
                                if kind == "manual" {
                                    if let Some(sid) = self.selected_id {
                                        db.add_track_to_playlist(pl.id, sid).ok();
                                    }
                                }
                                // F-18: colgarla de su carpeta padre
                                if let Some(pid) = parent {
                                    if db.set_playlist_parent(pl.id, Some(pid)).is_ok() {
                                        // asegurar la carpeta expandida para
                                        // ver la nueva sub-playlist
                                        self.playlist_collapsed.remove(&pid);
                                    }
                                }
                            }
                        }
                        self.new_playlist_parent = None;
                        self.refresh();
                        self.show_new_playlist = false;
                    }
                });
            self.show_new_playlist = open;
        }
        if self.show_rename_playlist {
            let mut open = true;
            egui::Window::new("Renombrar playlist")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut self.new_playlist_name);
                    ui.horizontal(|ui| {
                        if ui.button("Guardar").clicked() && !self.new_playlist_name.is_empty() {
                            if let Some(pid) = self.rename_playlist_id {
                                if let Ok(db) = self.db.lock() {
                                    let _ = db.rename_playlist(pid, &self.new_playlist_name);
                                }
                            }
                            self.refresh();
                            self.show_rename_playlist = false;
                        }
                        if ui.button("Cancelar").clicked() {
                            self.show_rename_playlist = false;
                        }
                    });
                });
            self.show_rename_playlist = open;
        }
        // F-19: panel de orden manual de la playlist abierta
        if self.manual_order {
            if let Some(pid) = self.active_playlist {
                let mut open = true;
                let pl_name = self
                    .playlists
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let t = Theme::default();
                let mut reload = false;
                egui::Window::new(format!(
                    "{} — {pl_name}",
                    texts::t(&self.prefs.language, "playlist.order.title")
                ))
                .default_width(440.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(texts::t(&self.prefs.language, "playlist.order.hint"))
                            .small()
                            .color(t.text_dim),
                    );
                    ui.add_space(4.0);
                    let rows = self.manual_order_rows.clone();
                    if rows.is_empty() {
                        ui.label(
                            egui::RichText::new(texts::t(
                                &self.prefs.language,
                                "playlist.order.empty",
                            ))
                            .color(t.text_dim),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            for (i, (_id, title, artist)) in rows.iter().enumerate() {
                                // la acción sale del closure con el valor de
                                // retorno de `horizontal` (patrón de la tabla)
                                let act = ui
                                    .horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{:>2}.", i + 1))
                                                .monospace()
                                                .color(t.text_dim),
                                        );
                                        if ui
                                            .add_enabled(i > 0, egui::Button::new("^").small())
                                            .on_hover_text(texts::t(
                                                &self.prefs.language,
                                                "playlist.up",
                                            ))
                                            .clicked()
                                        {
                                            return Some((i, true));
                                        }
                                        if ui
                                            .add_enabled(
                                                i + 1 < rows.len(),
                                                egui::Button::new("v").small(),
                                            )
                                            .on_hover_text(texts::t(
                                                &self.prefs.language,
                                                "playlist.down",
                                            ))
                                            .clicked()
                                        {
                                            return Some((i, false));
                                        }
                                        ui.label(
                                            egui::RichText::new(format!("{title} — {artist}"))
                                                .color(t.text),
                                        );
                                        None
                                    })
                                    .inner;
                                if let Some((idx, up)) = act {
                                    let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
                                    if let Some(new_order) =
                                        crate::playlists::swap_step(&ids, idx, up)
                                    {
                                        if let Ok(db) = self.db.lock() {
                                            if db.reorder_playlist(pid, &new_order).is_ok() {
                                                eprintln!(
                                                    "[f19] reorder playlist {pid}: pista {} {}",
                                                    if up { "sube" } else { "baja" },
                                                    idx + 1
                                                );
                                            }
                                        }
                                        reload = true;
                                    }
                                }
                            }
                        });
                });
                self.manual_order = open;
                if reload {
                    self.reload_manual_order();
                    self.refresh();
                }
            } else {
                // la playlist activa desapareció (borrada): salir del modo
                self.manual_order = false;
            }
        }
        if self.show_export {
            let mut open = true;
            let mut log = self.export_log.clone();
            egui::Window::new(texts::t(&self.prefs.language, "export.title"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(texts::t(&self.prefs.language, "export.hint"));
                    ui.add_space(4.0);
                    ui.checkbox(&mut self.prefs.export_traktor, "Traktor NML");
                    ui.checkbox(&mut self.prefs.export_serato, "Serato crates");
                    ui.checkbox(&mut self.prefs.export_rekordbox, "Rekordbox XML");
                    ui.checkbox(&mut self.prefs.export_engine, "Engine DJ (estructura USB)");
                    ui.checkbox(&mut self.prefs.export_vdj, "VirtualDJ (playlist + tags)");
                    ui.checkbox(
                        &mut self.prefs.write_tags_id3,
                        "Escribir tags ID3 (COMM/TKEY/TBPM)",
                    );
                    ui.checkbox(
                        &mut self.prefs.export_serato_cues,
                        "Serato cues EN ARCHIVOS (GEOB Markers2 — modifica el audio)",
                    );
                    ui.horizontal(|ui| {
                        ui.label("formato de comment:");
                        ui.text_edit_singleline(&mut self.prefs.id3_comment_format);
                    });
                    ui.add_space(6.0);
                    // F-25: destino de exportación — carpeta local (comportamiento
                    // actual) o unidad USB removible (exporta a <unidad>:/SmartMix/)
                    ui.horizontal(|ui| {
                        ui.label(texts::t(&self.prefs.language, "export.dest"));
                        // fix usb-scan: mientras el scan corre en background el
                        // combo muestra "Buscando…" + spinner y el botón queda
                        // deshabilitado (antes: powershell SINCRÓNICO en el
                        // hilo UI → ventana congelada ~1.5 s, "no responde").
                        let scanning = self.device_scan.is_some();
                        let sel = self.export_dest.min(self.usb_drives.len());
                        let selected = if scanning {
                            texts::t(&self.prefs.language, "export.dest.scanning")
                        } else if sel == 0 {
                            texts::t(&self.prefs.language, "export.dest.local")
                        } else {
                            usb_dest_label(&self.usb_drives[sel - 1])
                        };
                        let drives = self.usb_drives.clone();
                        egui::ComboBox::from_id_source("export-dest")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.export_dest,
                                    0,
                                    texts::t(&self.prefs.language, "export.dest.local"),
                                );
                                for (i, d) in drives.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.export_dest,
                                        i + 1,
                                        usb_dest_label(d),
                                    );
                                }
                            });
                        if ui
                            .add_enabled(
                                !scanning,
                                egui::Button::new(texts::t(
                                    &self.prefs.language,
                                    "export.dest.refresh",
                                )),
                            )
                            .clicked()
                        {
                            // hilo de fondo (App::spawn_device_scan); el
                            // resultado lo drena update() — jamás bloquear aquí.
                            self.spawn_device_scan();
                        }
                        if scanning {
                            ui.add_space(4.0);
                            ui.spinner();
                        }
                    });
                    let sel = self.export_dest.min(self.usb_drives.len());
                    if sel > 0 {
                        let d = self.usb_drives[sel - 1].clone();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}",
                                texts::t(&self.prefs.language, "export.dest.usb_note"),
                                sm_usb::smartmix_dir(&d.root).display()
                            ))
                            .small()
                            .color(t.text_dim),
                        );
                        if ui
                            .button(texts::t(&self.prefs.language, "export.run.usb"))
                            .clicked()
                        {
                            let dir = sm_usb::smartmix_dir(&d.root);
                            match std::fs::create_dir_all(&dir) {
                                Ok(_) => log = self.do_export(&dir, Some(&d)),
                                Err(e) => log = format!("✗ USB: {e}"),
                            }
                        }
                    } else if ui.button("Exportar vista actual…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            log = self.do_export(&dir, None);
                        }
                    }
                    if ui.button("Renombrar archivos (F-30)…").clicked() {
                        self.rename_report.clear();
                        self.rename_plan.clear();
                        self.show_rename_files = true;
                    }
                    if !log.is_empty() {
                        ui.add_space(6.0);
                        ui.monospace(&log);
                    }
                });
            self.export_log = log;
            self.show_export = open;
        }
        if self.show_rename_files {
            let mut open = true;
            egui::Window::new("Renombrar archivos (F-30)")
                .open(&mut open)
                .default_width(620.0)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Renombra los ARCHIVOS de la vista actual al estilo herramienta de referencia. \
                             Previsualiza antes de tocar nada; nunca sobrescribe.",
                        )
                        .small()
                        .color(t.text_dim),
                    );
                    ui.add_space(4.0);
                    egui::ComboBox::from_id_source("rename-pattern")
                        .selected_text(
                            [
                                "herramienta de referencia: Key_Mode_-_Tempo_-_Title_-_Artist",
                                "Camelot: 7A_-_Tempo…",
                                "Personalizado",
                            ][self.rename_pattern_idx],
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.rename_pattern_idx,
                                0,
                                "herramienta de referencia: Key_Mode_-_Tempo_-_Title_-_Artist",
                            );
                            ui.selectable_value(
                                &mut self.rename_pattern_idx,
                                1,
                                "Camelot: 7A_-_Tempo…",
                            );
                            ui.selectable_value(&mut self.rename_pattern_idx, 2, "Personalizado");
                        });
                    if self.rename_pattern_idx == 2 {
                        ui.text_edit_singleline(&mut self.rename_custom);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("1 · Previsualizar").clicked() {
                            self.build_rename_plan();
                        }
                        let enabled = !self.rename_plan.is_empty() && self.rename_report.is_empty();
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(
                                    egui::RichText::new(format!(
                                        "2 · APLICAR {} renombrados",
                                        self.rename_plan.len()
                                    ))
                                    .strong(),
                                ),
                            )
                            .clicked()
                        {
                            self.apply_rename_plan(ctx);
                        }
                    });
                    if !self.rename_report.is_empty() {
                        ui.add_space(4.0);
                        ui.monospace(&self.rename_report.clone());
                    } else if !self.rename_plan.is_empty() {
                        ui.add_space(4.0);
                        ui.label(format!(
                            "{} renombrados planificados:",
                            self.rename_plan.len()
                        ));
                        egui::ScrollArea::vertical()
                            .max_height(180.0)
                            .show(ui, |ui| {
                                for r in self.rename_plan.iter().take(200) {
                                    let fname = |p: &std::path::Path| {
                                        p.file_name()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default()
                                    };
                                    ui.monospace(format!("{} → {}", fname(&r.from), fname(&r.to)));
                                }
                            });
                        if self.rename_plan.len() > 200 {
                            ui.label(format!("… y {} más", self.rename_plan.len() - 200));
                        }
                    }
                });
            self.show_rename_files = open;
        }
        if self.show_safe_format {
            let mut open = true;
            egui::Window::new("⚡ Safe Format — USB")
                .open(&mut open)
                .default_width(640.0)
                .show(ctx, |ui| {
                    let Some((root, dev, size_gb)) = self.sf_drive.clone() else {
                        ui.label("Sin unidad seleccionada.");
                        return;
                    };
                    ui.label(
                        egui::RichText::new(format!("Unidad {dev} ({root:?}) · {size_gb:.1} GB"))
                            .strong(),
                    );
                    // Pipeline como LISTA numerada (antes un párrafo inline
                    // con flechas): cada acción en su línea, mismas palabras.
                    // Layout only — la lógica de fases no cambia.
                    crate::studio::section_label(ui, "CÓMO FUNCIONA");
                    for paso in [
                        "1 · Backup verificado SHA-256",
                        "2 · FORMATO FAT32 (BORRA TODO)",
                        "3 · Restore automático + verificación",
                    ] {
                        ui.label(paso);
                    }
                    ui.label(
                        egui::RichText::new(
                            "Si algo falla antes del formato, la unidad queda intacta.",
                        )
                        .small()
                        .color(t.text_dim),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Destino del backup:");
                        if ui.button("Elegir…").clicked() {
                            if let Some(d) = rfd::FileDialog::new()
                                .set_title("Destino del backup PREVIO al formato")
                                .pick_folder()
                            {
                                self.sf_dest = Some(d);
                            }
                        }
                        ui.label(
                            self.sf_dest
                                .as_ref()
                                .map(|d| d.display().to_string())
                                .unwrap_or_else(|| "— pendiente —".into()),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Etiqueta post-formato (≤11):");
                        ui.add(egui::TextEdit::singleline(&mut self.sf_label).desired_width(90.0));
                    });
                    ui.add_space(4.0);
                    // paso 2: dry-run
                    let dry_enabled = self.sf_dest.is_some()
                        && matches!(self.sf_phase, SfPhase::Idle | SfPhase::Ready);
                    if ui
                        .add_enabled(
                            dry_enabled,
                            egui::Button::new("1 · Analizar unidad (sin tocar nada)"),
                        )
                        .clicked()
                    {
                        self.sf_start_dry();
                    }
                    // paso 3: confirmación doble — plan en SECCIONES (antes
                    // todo inline: el comando y los avisos en un solo
                    // párrafo ilegible). Solo layout; fases y textos intactos.
                    if matches!(self.sf_phase, SfPhase::Ready) {
                        ui.add_space(4.0);
                        let dest = self
                            .sf_dest
                            .as_ref()
                            .map(|d| d.display().to_string())
                            .unwrap_or_else(|| "—".into());
                        crate::studio::section_label(ui, "SE BORRARÁ");
                        ui.label(crate::studio::small(
                            format!("· Todo el contenido de {dev}: FORMATO FAT32 (BORRA TODO)"),
                            t.text_dim,
                        ));
                        crate::studio::section_label(ui, "SE CONSERVARÁ");
                        ui.label(crate::studio::small(
                            format!("· Backup verificado SHA-256 en {dest}"),
                            t.text_dim,
                        ));
                        ui.label(crate::studio::small(
                            "· Restore automático + verificación tras el formato",
                            t.text_dim,
                        ));
                        if !self.sf_notes.is_empty() {
                            crate::studio::section_label(ui, "AVISOS");
                            for n in &self.sf_notes {
                                ui.label(crate::studio::small(format!("· {n}"), t.text_dim));
                            }
                        }
                        crate::studio::section_label(ui, "COMANDO");
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}",
                                self.sf_cmd,
                                if self.sf_admin {
                                    "  (requiere admin)"
                                } else {
                                    ""
                                }
                            ))
                            .monospace()
                            .small()
                            .color(t.text_dim),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Para EJECUTAR escribe el token {} DOS veces:",
                                self.sf_token
                            ))
                            .strong(),
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.sf_tok_a).desired_width(150.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.sf_tok_b).desired_width(150.0),
                            );
                        });
                        let confirmed = !self.sf_token.is_empty()
                            && self.sf_tok_a == self.sf_token
                            && self.sf_tok_b == self.sf_token;
                        if ui
                            .add_enabled(
                                confirmed,
                                egui::Button::new(
                                    egui::RichText::new(
                                        "2 · EJECUTAR FORMATO (irreversible tras el backup)",
                                    )
                                    .strong()
                                    .color(egui::Color32::WHITE),
                                ),
                            )
                            .on_hover_text("backup → verificación → formato FAT32 → restore")
                            .clicked()
                        {
                            self.sf_start_run();
                        }
                    }
                    if !self.sf_log.is_empty() {
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                ui.monospace(&self.sf_log.clone());
                            });
                    }
                });
            self.show_safe_format = open;
        }
        if self.show_about {
            let open = true;
            egui::Window::new("Smart Mix").show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("SMART MIX")
                        .font(crate::theme::font_display(24.0))
                        .color(t.accent),
                );
                ui.label("Análisis armónico profesional para DJs.");
                ui.label("Motor: Harmonic Engine (port Rust nativo).");
                ui.label("© 2026 Avant Garden Music. Identidad visual propia.");
                ui.label(format!(
                    "Modelos ONNX: {}",
                    if self.models.available() {
                        "activos"
                    } else {
                        "fallback"
                    }
                ));
                if let Some(p) = &self.player {
                    ui.label(format!("Audio: {} @ {} Hz", p.device_name, p.sample_rate));
                }
            });
            self.show_about = open;
        }
        // correcciones (antes panel derecho)
        if self.show_corrections {
            let mut open = true;
            egui::Window::new("Correcciones")
                .open(&mut open)
                .show(ctx, |ui| {
                    let Some(full) = self.detail.clone() else {
                        ui.label("Selecciona una pista primero.");
                        return;
                    };
                    let track_id = full.id;
                    ui.label(egui::RichText::new(&full.meta.title).font(font_heading(15.0)));
                    ui.separator();
                    // override key/bpm
                    ui.label(
                        egui::RichText::new("Tonalidad / BPM manuales")
                            .font(font_heading(15.0))
                            .color(t.text_dim),
                    );
                    let cur_key = full.analysis.key_camelot.clone();
                    let cur_bpm = full.analysis.tempo;
                    let mut apply_key: Option<Option<String>> = None;
                    let mut apply_bpm: Option<f64> = None;
                    let mut do_apply = false;
                    ui.horizontal(|ui| {
                        let sel = cur_key.clone().unwrap_or_default();
                        egui::ComboBox::from_id_source("ovr_key")
                            .selected_text(if sel.is_empty() { "—".into() } else { sel })
                            .width(58.0)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(cur_key.is_none(), "—").clicked() {
                                    apply_key = Some(None);
                                }
                                for n in 1..=12u8 {
                                    for l in ["A", "B"] {
                                        let k = format!("{n}{l}");
                                        if ui
                                            .selectable_label(
                                                cur_key.as_deref() == Some(k.as_str()),
                                                &k,
                                            )
                                            .clicked()
                                        {
                                            apply_key = Some(Some(k));
                                        }
                                    }
                                }
                            });
                        let mut b = cur_bpm.unwrap_or(128.0);
                        if ui
                            .add(egui::DragValue::new(&mut b).speed(0.1).fixed_decimals(2))
                            .changed()
                        {
                            apply_bpm = Some(b);
                        }
                        if ui.button("Aplicar").clicked() {
                            do_apply = true;
                        }
                    });
                    if do_apply && (apply_key.is_some() || apply_bpm.is_some()) {
                        if let Ok(mut db) = self.db.lock() {
                            let _ = db.override_key_bpm(
                                track_id,
                                apply_key.clone().flatten().as_deref(),
                                apply_bpm.or(cur_bpm),
                            );
                        }
                        self.load_detail(track_id);
                        self.refresh();
                    }
                    // beatgrid
                    if full.analysis.beatgrid.is_some() {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("Beatgrid")
                                .font(font_heading(15.0))
                                .color(t.text_dim),
                        );
                        let mut act: Option<&str> = None;
                        ui.horizontal(|ui| {
                            if ui.small_button("◀ 10 ms").clicked() {
                                act = Some("left");
                            }
                            if ui.small_button("10 ms ▶").clicked() {
                                act = Some("right");
                            }
                            if ui.small_button("⚓ playhead").clicked() {
                                act = Some("anchor");
                            }
                        });
                        if let Some(a) = act {
                            let mut an = full.analysis.clone();
                            if let Some(bg) = an.beatgrid.as_mut() {
                                match a {
                                    "left" => bg.t0_s -= 0.010,
                                    "right" => bg.t0_s += 0.010,
                                    "anchor" => {
                                        let ph = self
                                            .player()
                                            .map(|p| p.decks[self.active_deck].position_s())
                                            .unwrap_or(0.0);
                                        let near = bg.beat_time(bg.beat_index_near(ph));
                                        bg.t0_s += ph - near;
                                    }
                                    _ => {}
                                }
                            }
                            if let Ok(mut db) = self.db.lock() {
                                let _ = db.save_analysis(track_id, &an);
                            }
                            self.load_detail(track_id);
                            self.refresh();
                        }
                    }
                    // acciones
                    ui.add_space(6.0);
                    ui.separator();
                    if ui.button("↻ Re-analizar").clicked() {
                        self.reanalyze_track(track_id);
                    }
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_source("addpl")
                            .selected_text("Añadir a playlist…")
                            .show_ui(ui, |ui| {
                                for pl in self.playlists.clone() {
                                    if ui.selectable_label(false, &pl.name).clicked() {
                                        if let Ok(mut db) = self.db.lock() {
                                            db.add_track_to_playlist(pl.id, track_id).ok();
                                        }
                                        self.status_msg = format!("Añadida a {}", pl.name);
                                    }
                                }
                            });
                        if let Some(k) = &full.analysis.key_camelot {
                            if ui
                                .button("Ver compatibles")
                                .on_hover_text(format!("compatibles con {k}"))
                                .clicked()
                            {
                                self.compat_key = Some(k.clone());
                                self.active_playlist = None;
                                self.collection = Collection::All;
                                self.refresh();
                            }
                        }
                    });
                });
            self.show_corrections = open;
        }
        // visor de crate USB (solo lectura)
        if let Some((name, entries)) = self.usb_crate_view.clone() {
            let mut open = true;
            egui::Window::new(format!("USB · crate «{name}»"))
                .open(&mut open)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} pistas · dispositivo SOLO LECTURA",
                            entries.len()
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    egui::ScrollArea::vertical().show_rows(ui, 18.0, entries.len(), |ui, range| {
                        for i in range {
                            ui.monospace(egui::RichText::new(&entries[i]).small());
                        }
                    });
                });
            if !open {
                self.usb_crate_view = None;
            }
        }
    }

    /// F-24 fase 2: vista Tag Editor — edición masiva estilo herramienta de referencia sobre las
    /// N pistas seleccionadas. Campo vacío = "sin cambio" para las N;
    /// Aplicar escribe SOLO los campos no vacíos (v1 sin opción de vaciar).
    pub(crate) fn tag_editor_view(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                egui::RichText::new("< VOLVER A LA BIBLIOTECA")
                                    .font(crate::theme::font_body(14.0))
                                    .color(t.cyan),
                            )
                            .clicked()
                        {
                            self.tags_view = false;
                        }
                    });
                    ui.add_space(8.0);
                    let n = self.tag_tracks.len();
                    ui.label(
                        egui::RichText::new("TAG EDITOR")
                            .font(crate::theme::font_heading(22.0))
                            .color(t.text),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{n} pista(s) — el campo vacío NO se toca (sin cambio)"
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    if n == 0 {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(
                                "Sin pistas: vuelve a la biblioteca y selecciona \
                                 con ctrl+clic, shift+clic o ✓TODO.",
                            )
                            .color(t.text_dim),
                        );
                        return;
                    }
                    ui.add_space(10.0);
                    // formulario de campos (TrackMeta: title/artist/album/
                    // genre/year — no hay campo Comment en el modelo v1)
                    let mut f = std::mem::take(&mut self.tag_fields);
                    egui::Grid::new("tag-fields")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            for (label, val) in [
                                ("Title", &mut f.title),
                                ("Artist", &mut f.artist),
                                ("Album", &mut f.album),
                                ("Genre", &mut f.genre),
                                ("Year", &mut f.year),
                            ] {
                                ui.label(
                                    egui::RichText::new(label)
                                        .font(font_heading(15.0))
                                        .color(t.text_dim),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(val)
                                        .desired_width(340.0)
                                        .hint_text("vacío = sin cambio"),
                                );
                                ui.end_row();
                            }
                        });
                    self.tag_fields = f;
                    // preview de las primeras 20 antes de aplicar
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(
                        egui::RichText::new("PREVIEW")
                            .font(font_heading(15.0))
                            .color(t.text_dim),
                    );
                    let fields = self.tag_fields.clone();
                    let changed = fields.changed_fields();
                    if changed.is_empty() {
                        ui.label(
                            egui::RichText::new("Ningún campo para aplicar (todo vacío).")
                                .small()
                                .color(t.text_dim),
                        );
                    } else {
                    // fase 3: escritura opcional de tags de archivo (OFF)
                    ui.add_space(6.0);
                    ui.checkbox(
                        &mut self.tag_write_files,
                        "escribir también tags de los archivos (mp3/aiff/wav/flac)",
                    )
                    .on_hover_text(
                        "F-24: escribe los campos editados en el tag del archivo \
                         con lofty. MODIFICA los archivos de audio; los errores \
                         por archivo se cuentan, no abortan el lote.",
                    );
                    // aplicar
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let enabled = n > 0 && self.tag_fields.any_set();
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(
                                    egui::RichText::new(format!("APLICAR A {n} PISTA(S)"))
                                        .strong(),
                                ),
                            )
                            .clicked()
                        {
                            self.tag_apply();
                        }
                        if ui.button("Recargar selección").clicked() {
                            self.tag_load_tracks();
                        }
                        // Cierre: cancelar cierra el editor DESCARTANDO los
                        // campos no aplicados (VOLVER los conserva).
                        if ui
                            .button(
                                egui::RichText::new(crate::texts::t(
                                    &self.prefs.language,
                                    "studio.cancel",
                                ))
                                .strong()
                                .color(t.cyan),
                            )
                            .on_hover_text(
                                "Cierra el editor descartando los campos aún no aplicados",
                            )
                            .clicked()
                        {
                            self.tag_fields = crate::tag_editor::TagEditFields::default();
                            self.tag_junk_scanned = false;
                            self.tag_junk_preview.clear();
                            self.tag_report.clear();
                            self.tags_view = false;
                        }
                    });
                        ui.label(
                            egui::RichText::new(format!(
                                "Se escribirá {} en {n} pista(s):",
                                changed.join(", ")
                            ))
                            .small()
                            .color(t.text),
                        );
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for (_id, meta) in self.tag_tracks.iter().take(20) {
                                    let line =
                                        crate::tag_editor::preview_line(meta, &fields);
                                    let name = if meta.title.is_empty() {
                                        std::path::Path::new(&meta.path)
                                            .file_stem()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default()
                                    } else {
                                        meta.title.clone()
                                    };
                                    ui.monospace(format!(
                                        "{name} · {}",
                                        if line.is_empty() {
                                            "sin cambios".into()
                                        } else {
                                            line
                                        }
                                    ));
                                }
                                if n > 20 {
                                    ui.label(
                                        egui::RichText::new(format!("… y {} más", n - 20))
                                            .small()
                                            .color(t.text_dim),
                                    );
                                }
                                // Cierre: aire inferior para que la última
                                // fila del preview quede ENTERA al llegar al
                                // final del scroll (antes quedaba recortada
                                // contra el borde del área).
                                ui.add_space(10.0);
                            });
                        ui.add_space(6.0);
                    }
                    // -------------------------------------------------------
                    // F-31: limpieza de tags basura (preview antes de aplicar)
                    // -------------------------------------------------------
                    ui.add_space(12.0);
                    ui.separator();
                    ui.label(
                        egui::RichText::new("LIMPIEZA DE TAGS BASURA")
                            .font(font_heading(15.0))
                            .color(t.text_dim),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Quita comments basura (tiendas, herramientas, claves) \
                             preservando título/artista/álbum/key/BPM/energía/cues. \
                             NUNCA toca los demás campos del tag.",
                        )
                        .small()
                        .color(t.text_dim),
                    );
                    ui.checkbox(
                        &mut self.tag_junk_gapless,
                        "incluir también iTunSMPB/TCMP (PELIGROSO para gapless)",
                    )
                    .on_hover_text(
                        "Opt-in explícito: borrar iTunSMPB puede romper la reproducción \
                         gapless de pistas que la usan (compilaciones de iTunes).",
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("PREVIEW BASURA").strong(),
                            ))
                            .on_hover_text("Lee los tags de la selección y lista qué se quitaría (sin escribir)")
                            .clicked()
                        {
                            self.tag_junk_scan();
                        }
                        let can_clean =
                            self.tag_junk_scanned && !self.tag_junk_preview.is_empty();
                        if ui
                            .add_enabled(
                                can_clean && self.tag_write_files,
                                egui::Button::new(
                                    egui::RichText::new("LIMPIAR ARCHIVOS").strong(),
                                ),
                            )
                            .on_hover_text(if self.tag_write_files {
                                "Borra los campos listados del tag de cada archivo (F-31)"
                            } else {
                                "Requiere 'escribir también tags de los archivos': la BD no guarda comments"
                            })
                            .clicked()
                        {
                            self.tag_junk_clean();
                        }
                    });
                    if !self.tag_write_files {
                        ui.label(
                            egui::RichText::new(
                                "La limpieza escribe en los ARCHIVOS: activa el checkbox \
                                 de escritura de arriba (mismo consentimiento que F-24).",
                            )
                            .small()
                            .color(t.text_dim),
                        );
                    }
                    if self.tag_junk_scanned {
                        if self.tag_junk_preview.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "Sin tags basura en la selección (con la política actual).",
                                )
                                .small()
                                .color(t.text_dim),
                            );
                        } else {
                            let nj = self.tag_junk_preview.len();
                            ui.label(
                                egui::RichText::new(format!(
                                    "Se quitaría basura de {nj} pista(s):"
                                ))
                                .small()
                                .color(t.text),
                            );
                            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                                for line in self.tag_junk_preview.iter().take(20) {
                                    ui.monospace(line.clone());
                                }
                                if nj > 20 {
                                    ui.label(
                                        egui::RichText::new(format!("… y {} más", nj - 20))
                                            .small()
                                            .color(t.text_dim),
                                    );
                                }
                                // Cierre: mismo aire inferior del preview principal
                                ui.add_space(10.0);
                            });
                        }
                    }
                    if !self.tag_junk_report.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&self.tag_junk_report).color(t.cyan));
                    }
                    if !self.tag_report.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&self.tag_report).color(t.cyan));
                    }
                });
            });
    }

    /// Settings/Export/Integrations (§11) como vista dedicada.
    pub(crate) fn settings_view(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            egui::RichText::new("< VOLVER A LA BIBLIOTECA")
                                .font(crate::theme::font_body(14.0))
                                .color(t.cyan),
                        )
                        .clicked()
                    {
                        self.settings_view = false;
                    }
                });
                ui.add_space(6.0);
                let mut tab = self.settings_tab;
                ui.horizontal(|ui| {
                    // nav rail §11 (280px)
                    egui::Frame::none()
                        .fill(t.surface2)
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(240.0);
                            for (i, name) in
                                ["GENERAL", "EXPORTAR", "INTEGRACIONES", "TAG SAFETY"].iter().enumerate()
                            {
                                let sel = tab == i as u8;
                                let resp = ui.selectable_value(
                                    &mut tab,
                                    i as u8,
                                    egui::RichText::new(*name)
                                        .font(crate::theme::font_body(15.0))
                                        .color(if sel { t.text } else { t.text_dim }),
                                );
                                if sel {
                                    let r = resp.rect;
                                    let bar = egui::Rect::from_min_size(
                                        egui::pos2(r.left(), r.top()),
                                        egui::vec2(3.0, r.height()),
                                    );
                                    ui.painter().rect_filled(bar, 0.0, t.cyan);
                                }
                            }
                        });
                    ui.add_space(12.0);
                    ui.vertical(|ui| match tab {
                        0 => {
                            ui.label(
                                egui::RichText::new("GENERAL")
                                    .font(crate::theme::font_heading(17.0))
                                    .color(t.text),
                            );
                            ui.add_space(8.0);
                            egui::ComboBox::from_id_source("lang2")
                                .selected_text(format!("idioma: {}", self.prefs.language))
                                .show_ui(ui, |ui| {
                                    for l in sm_i18n::I18n::available() {
                                        if ui
                                            .selectable_label(
                                                self.prefs.language == l,
                                                l.to_uppercase(),
                                            )
                                            .clicked()
                                        {
                                            self.prefs.language = l.to_string();
                                        }
                                    }
                                });
                            // F-25: tema claro — aplica EN VIVO (paleta global +
                            // reinstall de visuals) y persiste al instante
                            let mut tl = self.prefs.theme_light;
                            let resp = ui.checkbox(
                                &mut tl,
                                texts::t(&self.prefs.language, "prefs.theme_light"),
                            );
                            if resp.changed() {
                                self.prefs.theme_light = tl;
                                crate::theme::set_light_mode(tl);
                                crate::theme::install(ctx);
                                ctx.request_repaint();
                                if let Ok(db) = self.db.lock() {
                                    let _ = db.set_preferences(&self.prefs);
                                }
                                self.status_msg = if tl {
                                    "Tema claro activado".into()
                                } else {
                                    "Tema oscuro activado".into()
                                };
                            }
                            ui.add(
                                egui::Slider::new(&mut self.prefs.analysis_workers, 1..=16)
                                    .text("hilos de análisis"),
                            );
                            ui.checkbox(&mut self.prefs.analyze_on_import, "Analizar al importar");
                            if ui.button("Guardar preferencias").clicked() {
                                if let Ok(db) = self.db.lock() {
                                    let _ = db.set_preferences(&self.prefs);
                                }
                                self.status_msg = "Preferencias guardadas".into();
                            }
                        }
                        1 => {
                            ui.label(
                                egui::RichText::new("EXPORTAR")
                                    .font(crate::theme::font_heading(17.0))
                                    .color(t.text),
                            );
                            ui.add_space(8.0);
                            // 4 tarjetas destino §11 (REKORDBOX/TRAKTOR/SERATO/ENGINE)
                            ui.horizontal(|ui| {
                                let cards = [
                                    ("REKORDBOX XML", &mut self.prefs.export_rekordbox),
                                    ("TRAKTOR NML", &mut self.prefs.export_traktor),
                                    ("SERATO", &mut self.prefs.export_serato),
                                    ("ENGINE DJ", &mut self.prefs.export_engine),
                                ];
                                let cw = (ui.available_width() - 36.0) / 4.0;
                                for (name, flag) in cards {
                                    egui::Frame::none()
                                        .fill(t.container)
                                        .rounding(12.0)
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0))
                                        .show(ui, |ui| {
                                            ui.set_min_width(cw);
                                            ui.set_min_height(96.0);
                                            ui.label(
                                                egui::RichText::new(name)
                                                    .font(crate::theme::font_heading(14.0))
                                                    .color(t.text),
                                            );
                                            ui.add_space(4.0);
                                            ui.checkbox(flag, if *flag { "activado" } else { "desactivado" });
                                        });
                                    ui.add_space(4.0);
                                }
                            });
                            ui.add_space(10.0);
                            // Export flow §11 (920×482)
                            egui::Frame::none()
                                .fill(t.container)
                                .rounding(12.0)
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("FLUJO DE EXPORTACIÓN")
                                            .font(crate::theme::font_heading(15.5))
                                            .color(t.text_dim),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Se exportará la vista actual ({} pistas) con cues y beatgrid a cada destino activado.",
                                            self.total
                                        ))
                                        .color(t.text_secondary),
                                    );
                                    ui.checkbox(&mut self.prefs.export_vdj, "Incluir VirtualDJ (playlist + tags)");
                                    ui.add_space(4.0);
                                    if ui.button("Elegir carpeta y exportar…").clicked() {
                                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                            self.export_log = self.do_export(&dir, None);
                                            self.show_export = false;
                                        }
                                    }
                                    if !self.export_log.is_empty() {
                                        ui.add_space(4.0);
                                        ui.monospace(&self.export_log);
                                    }
                                });
                        }
                        2 => {
                            ui.label(
                                egui::RichText::new("INTEGRACIONES")
                                    .font(crate::theme::font_heading(17.0))
                                    .color(t.text),
                            );
                            ui.add_space(8.0);
                            for (name, on) in [
                                ("Rekordbox", self.prefs.export_rekordbox),
                                ("Traktor", self.prefs.export_traktor),
                                ("Serato", self.prefs.export_serato),
                                ("Engine DJ", self.prefs.export_engine),
                                ("VirtualDJ", self.prefs.export_vdj),
                            ] {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{name}: {}",
                                        if on { "activo" } else { "inactivo" }
                                    ))
                                    .color(if on { t.success } else { t.text_dim }),
                                );
                            }
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(
                                    "USB: escritura directa con backup pendiente de autorización explícita.",
                                )
                                .small()
                                .color(t.orange),
                            );
                        }
                        _ => {
                            ui.label(
                                egui::RichText::new("TAG SAFETY")
                                    .font(crate::theme::font_heading(17.0))
                                    .color(t.text),
                            );
                            ui.add_space(8.0);
                            ui.checkbox(
                                &mut self.prefs.write_tags_id3,
                                "Permitir escritura de tags ID3 (COMM/TKEY/TBPM)",
                            );
                            ui.horizontal(|ui| {
                                ui.label("formato:");
                                ui.text_edit_singleline(&mut self.prefs.id3_comment_format);
                            });
                            ui.add_space(6.0);
                            // preview antes→después con la pista seleccionada (real)
                            if let Some(full) = &self.detail {
                                if let (Some(k), Some(b)) =
                                    (&full.analysis.key_camelot, full.analysis.tempo)
                                {
                                    match sm_export::preview_id3(
                                        &full.meta.path,
                                        k,
                                        b,
                                        full.analysis.energy,
                                        &self.prefs.id3_comment_format,
                                    ) {
                                        Ok(p) => {
                                            ui.label(
                                                egui::RichText::new("vista previa (sin escribir):")
                                                    .small()
                                                    .color(t.text_dim),
                                            );
                                            ui.monospace(&p);
                                        }
                                        Err(e) => {
                                            ui.label(
                                                egui::RichText::new(format!("sin preview: {e}"))
                                                    .small()
                                                    .color(t.orange),
                                            );
                                        }
                                    }
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(
                                        "selecciona una pista para la vista previa",
                                    )
                                    .small()
                                    .color(t.text_dim),
                                );
                            }
                        }
                    });
                });
                self.settings_tab = tab;
            });
    }

    pub(crate) fn settings_view_panel(&mut self, ctx: &egui::Context) {
        self.settings_view(ctx);
    }

    /// F-30: construye el plan de renombrado de la vista actual (sin disco).
    pub(crate) fn build_rename_plan(&mut self) {
        self.rename_report.clear();
        self.rename_plan.clear();
        let Ok(db) = self.db.lock() else {
            self.rename_report = "✗ sin acceso a BD".into();
            return;
        };
        let mut q = self.query();
        q.limit = self.total as usize;
        q.offset = 0;
        let Ok(rows) = db.query(&q) else {
            self.rename_report = "✗ error de query".into();
            return;
        };
        let mut entries: Vec<(std::path::PathBuf, sm_export::RenameInfo)> = Vec::new();
        for r in &rows {
            if let Some(f) = db.track_full(r.id).unwrap_or(None) {
                entries.push((
                    std::path::PathBuf::from(&f.meta.path),
                    sm_export::RenameInfo {
                        title: f.meta.title.clone(),
                        artist: f.meta.artist.clone(),
                        key_camelot: f.analysis.key_camelot.clone().unwrap_or_default(),
                        bpm: f.analysis.tempo,
                    },
                ));
            }
        }
        let pattern = match self.rename_pattern_idx {
            0 => sm_export::RenamePattern::KeyBpmStyle,
            1 => sm_export::RenamePattern::CamelotSimple,
            _ => sm_export::RenamePattern::Custom(self.rename_custom.clone()),
        };
        let refs: Vec<(&std::path::Path, &sm_export::RenameInfo)> =
            entries.iter().map(|(p, i)| (p.as_path(), i)).collect();
        match sm_export::plan_renames(&refs, &pattern, None) {
            Ok(plan) => self.rename_plan = plan,
            Err(e) => self.rename_report = format!("✗ plan: {e}"),
        }
    }

    /// F-30: aplica el plan y sincroniza las rutas en la BD.
    pub(crate) fn apply_rename_plan(&mut self, ctx: &egui::Context) {
        let plan = std::mem::take(&mut self.rename_plan);
        match sm_export::apply_renames(&plan, |_| {}) {
            Ok(rep) => {
                let mut upd_ok = 0usize;
                if let Ok(mut db) = self.db.lock() {
                    for r in &plan {
                        if db
                            .update_track_path(
                                &r.from.display().to_string(),
                                &r.to.display().to_string(),
                            )
                            .unwrap_or(false)
                        {
                            upd_ok += 1;
                        }
                    }
                }
                self.rename_report = format!(
                    "✔ aplicados {} · conflictos saltados {} · errores {} · rutas BD actualizadas {}",
                    rep.applied,
                    rep.skipped_conflict,
                    rep.errors.len(),
                    upd_ok
                );
                self.refresh();
            }
            Err(e) => self.rename_report = format!("✗ apply: {e}"),
        }
        ctx.request_repaint();
    }

    /// Backup USB verificado (scan→backup→verify) en hilo de fondo.
    pub(crate) fn start_usb_backup(&mut self, root: std::path::PathBuf, dest: std::path::PathBuf) {
        let state = Arc::new(Mutex::new(UsbBackupState::default()));
        self.usb_backup = Some(state.clone());
        std::thread::spawn(move || {
            let st = state.clone();
            let mut prog = move |msg: &str, pct: f64| {
                if let Ok(mut s) = st.lock() {
                    s.lines.push(format!("[{:3.0}%] {msg}", pct * 100.0));
                }
            };
            let finish = |state: &Arc<Mutex<UsbBackupState>>, last: String| {
                if let Ok(mut s) = state.lock() {
                    s.lines.push(last);
                    s.done = true;
                }
            };
            match sm_usb::scan_drive(&root, Some(&mut prog)) {
                Ok(scan) => {
                    let res = sm_usb::backup_folder(&scan, &dest, Some(&mut prog));
                    let mism = sm_usb::verify_backup(&res, Some(&mut prog));
                    let last = format!(
                        "backup {}: {} archivos, {} fallidos{} — destino {}",
                        if res.ok() { "OK" } else { "CON FALLOS" },
                        res.manifest.len(),
                        res.failed.len(),
                        if mism.is_empty() {
                            String::new()
                        } else {
                            format!(" · verificación: {} desajustes", mism.len())
                        },
                        dest.display()
                    );
                    finish(&state, last);
                }
                Err(e) => finish(&state, format!("✗ escaneo: {e}")),
            }
        });
    }
}
