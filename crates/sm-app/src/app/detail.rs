use super::*;

impl SmartMixApp {
    /// Track Detail (§10): header + waveform avanzado + transporte +
    /// tarjetas TEMPO&BEATGRID / TONALIDAD&CONFIANZA / ENERGÍA&LOUDNESS /
    /// CUE POINTS. Todos los datos son reales del análisis (nada inventado).
    pub(crate) fn track_detail(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        let Some(id) = self.detail_view else { return };
        if self.selected_id != Some(id) {
            self.select(id);
        }
        let Some(full) = self.detail.clone() else {
            return;
        };
        let deck_handles = self.player.as_ref().map(|p| p.decks.clone());
        let deck = deck_handles.as_ref().map(|d| &d[self.active_deck]);
        let track_loaded = deck.map(|d| d.path() == full.meta.path).unwrap_or(false);
        let pos = if track_loaded {
            deck.map(|d| d.position_s())
        } else {
            None
        };
        let playing = track_loaded && deck.map(|d| d.is_playing()).unwrap_or(false);
        let duration = if track_loaded {
            deck.map(|d| d.duration_s())
                .unwrap_or(full.analysis.duration_s)
        } else {
            full.analysis.duration_s
        };
        let bpm = full.analysis.tempo;
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
                            self.detail_view = None;
                        }
                    });
                    ui.add_space(8.0);
                    // Detail header (§10: 76px): título Track + artista
                    ui.label(
                        egui::RichText::new(if full.meta.title.is_empty() {
                            "(sin título)"
                        } else {
                            &full.meta.title
                        })
                        .font(crate::theme::font_heading(26.0))
                        .color(t.text),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{} — {}",
                            if full.meta.artist.is_empty() {
                                "—"
                            } else {
                                &full.meta.artist
                            },
                            if full.meta.album.is_empty() {
                                "—"
                            } else {
                                &full.meta.album
                            },
                        ))
                        .color(t.text_dim),
                    );
                    ui.add_space(4.0);
                    // meta: badge A/B + BPM + dur + energía
                    ui.horizontal(|ui| {
                        if let Some(k) = &full.analysis.key_camelot {
                            let r = ui
                                .allocate_exact_size(egui::vec2(44.0, 22.0), Sense::hover())
                                .1
                                .rect;
                            ui.painter().rect_filled(r, 4.0, t.container);
                            ui.painter().text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                k,
                                egui::FontId::monospace(13.0),
                                crate::theme::key_label_color(k),
                            );
                        }
                        if let Some(b) = bpm {
                            ui.label(
                                egui::RichText::new(format!("{b:.2} BPM"))
                                    .monospace()
                                    .strong()
                                    .color(t.text),
                            );
                        }
                        if let Some(e) = full.analysis.energy {
                            ui.label(
                                egui::RichText::new(format!("E {e:.0}/10"))
                                    .monospace()
                                    .color(t.orange),
                            );
                        }
                        ui.label(
                            egui::RichText::new(fmt_time(duration))
                                .monospace()
                                .color(t.text_dim),
                        );
                        // F-26: rating editable (estrellas con painter)
                        {
                            let cur = full.rating;
                            let r_area = ui
                                .allocate_exact_size(egui::vec2(92.0, 20.0), Sense::hover())
                                .1
                                .rect;
                            let rects = crate::rating::star_rects(r_area, 13.0, 5.0);
                            for (r, on) in rects.iter().zip(crate::rating::star_states(cur)) {
                                if on {
                                    ui.painter().circle_filled(r.center(), 6.5, t.acid);
                                } else {
                                    ui.painter().circle_stroke(
                                        r.center(),
                                        6.0,
                                        egui::Stroke::new(1.4, t.border),
                                    );
                                }
                            }
                            let group = egui::Rect::from_min_max(rects[0].min, rects[4].max);
                            let resp = ui.interact(
                                group,
                                egui::Id::new("detail-rating").with(id),
                                Sense::click(),
                            );
                            let resp = resp.on_hover_text(
                                "rating: clic en la n-ésima estrella; clic en la actual quita",
                            );
                            if resp.clicked() {
                                if let Some(pos) = resp.interact_pointer_pos() {
                                    if let Some(n) = crate::rating::hit_star(&rects, pos) {
                                        let val = crate::rating::next_rating(cur, n);
                                        if let Ok(db) = self.db.lock() {
                                            if db.set_rating(id, val).is_ok() {
                                                eprintln!("[f26] detail rating id={id} -> {val}");
                                            }
                                        }
                                        if let Some(f) = self.detail.as_mut() {
                                            if f.id == id {
                                                f.rating = val;
                                            }
                                        }
                                        if let Some(row) =
                                            self.rows.values_mut().find(|r| r.id == id)
                                        {
                                            row.rating = val;
                                        }
                                    }
                                }
                            }
                        }
                    });
                    ui.add_space(10.0);
                    // Advanced waveform (§10: 1560×250 → escala de ventana)
                    let wave = full.analysis.waveform.clone();
                    let bands3 = full.analysis.waveform_bands.clone();
                    let cues: Vec<f64> = full.analysis.cues.iter().map(|c| c.time_s).collect();
                    let segs = full.analysis.segments.clone();
                    let mut seek_tx: Option<f64> = None;
                    let zkey = (Some(id), (duration * 20.0).round() as i64);
                    if self.wave_zoom_key != zkey {
                        self.wave_zoom = (0.0, duration.max(0.001));
                        self.wave_zoom_key = zkey;
                    }
                    let zoom = &mut self.wave_zoom;
                    // F-25: lienzo propio oscuro (bandas claras fijas, ídem deck)
                    egui::Frame::none()
                        .fill(crate::theme::WAVE_BG)
                        .show(ui, |ui| {
                            ui.add(&mut WaveView {
                                zoom,
                                wave: &wave,
                                bands: bands3.as_deref(),
                                duration_s: duration.max(0.001),
                                position_s: pos,
                                cues: &cues,
                                segments: &segs,
                                height: 150.0,
                                on_seek: &mut |tt| seek_tx = Some(tt),
                            });
                        });
                    if let Some(tt) = seek_tx {
                        if let Some(p) = deck_handles.as_ref() {
                            let d = &p[self.active_deck];
                            if !d.is_loaded() || d.path() != full.meta.path {
                                d.load(&full.meta.path);
                                self.deck_track_id[self.active_deck] = Some(id);
                            }
                            d.seek(tt);
                        }
                        ctx.request_repaint_after(std::time::Duration::from_millis(16));
                    }
                    // Detail transport (§10: 48px) — estilo CP-Playback-Controls
                    ui.add_space(6.0);
                    egui::Frame::none()
                        .fill(t.lowest)
                        .stroke(egui::Stroke::new(1.0, t.border))
                        .rounding(8.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dl = if self.active_deck == 0 { 'A' } else { 'B' };
                                ui.label(
                                    egui::RichText::new(format!(
                                        "DECK {dl} • {}",
                                        bpm.map(|b| format!("{b:.2}"))
                                            .unwrap_or_else(|| "—".into())
                                    ))
                                    .monospace()
                                    .color(t.text),
                                );
                                ui.add_space(16.0);
                                if let Some(p) = deck_handles.as_ref() {
                                    let d = &p[self.active_deck];
                                    // ⏮ volver al inicio
                                    if ui.button("⏮").clicked() && d.is_loaded() {
                                        d.seek(0.0);
                                    }
                                    // ▶/⏸ — botón principal (amarillo de marca)
                                    let play_btn = egui::Button::new(
                                        egui::RichText::new(if playing { "⏸" } else { "▶" })
                                            .strong()
                                            .color(if playing { t.text } else { t.acid }),
                                    );
                                    if ui.add(play_btn).clicked() {
                                        if playing {
                                            d.pause();
                                        } else if d.is_loaded() {
                                            d.play();
                                        } else {
                                            let path = full.meta.path.clone();
                                            d.load(&path);
                                            d.play();
                                            self.deck_track_id[self.active_deck] = Some(id);
                                        }
                                    }
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} / {}",
                                            fmt_time(pos.unwrap_or(0.0)),
                                            fmt_time(duration)
                                        ))
                                        .monospace()
                                        .color(t.magenta),
                                    );
                                    // SEEK (§9)
                                    let mut pos_seek = pos.unwrap_or(0.0);
                                    let sr = ui.add_sized(
                                        [110.0, 16.0],
                                        egui::Slider::new(&mut pos_seek, 0.0..=duration.max(0.01))
                                            .show_value(false)
                                            .text("SEEK"),
                                    );
                                    if sr.changed() {
                                        d.seek(pos_seek);
                                        ctx.request_repaint_after(
                                            std::time::Duration::from_millis(16),
                                        );
                                    }
                                }
                                // ⏭ siguiente pista de la vista actual
                                if ui.button("⏭").clicked() {
                                    if let Some(cur) = self
                                        .rows
                                        .values()
                                        .position(|r| Some(r.id) == self.detail_view)
                                    {
                                        let mut next = None;
                                        for k in (cur + 1)..self.rows.len() {
                                            if let Some(r) = self.rows.get(&k) {
                                                next = Some(r.id);
                                                break;
                                            }
                                        }
                                        if let Some(nid) = next {
                                            self.select(nid);
                                            self.detail_view = Some(nid);
                                            self.deck_play(self.active_deck);
                                        }
                                    }
                                }
                            });
                        });
                    ui.add_space(12.0);
                    // tarjetas (§10): 3 en fila + CUE POINTS ancha
                    ui.horizontal(|ui| {
                        let cw = (ui.available_width() - 24.0) / 3.0;
                        // TEMPO & BEATGRID
                        egui::Frame::none()
                            .fill(t.container)
                            .rounding(12.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_min_width(cw);
                                ui.label(card_title("TEMPO & BEATGRID"));
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}",
                                        bpm.map(|b| format!("{b:.2}"))
                                            .unwrap_or_else(|| "—".into())
                                    ))
                                    .monospace()
                                    .strong()
                                    .color(t.text),
                                );
                                ui.label(egui::RichText::new("BPM").small().color(t.text_dim));
                                if let Some(bg) = &full.analysis.beatgrid {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "t₀ {:.3}s · período {:.4}s · {} beats",
                                            bg.t0_s, bg.period_s, bg.n_beats
                                        ))
                                        .monospace()
                                        .small()
                                        .color(t.text_dim),
                                    );
                                }
                            });
                        // TONALIDAD & CONFIANZA
                        egui::Frame::none()
                            .fill(t.container)
                            .rounding(12.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_min_width(cw);
                                ui.label(card_title("TONALIDAD & CONFIANZA"));
                                ui.add_space(4.0);
                                if let Some(k) = &full.analysis.key_camelot {
                                    let r = ui
                                        .allocate_exact_size(egui::vec2(56.0, 26.0), Sense::hover())
                                        .1
                                        .rect;
                                    ui.painter().rect_filled(r, 4.0, t.container_high);
                                    ui.painter().text(
                                        r.center(),
                                        egui::Align2::CENTER_CENTER,
                                        k,
                                        egui::FontId::monospace(15.0),
                                        crate::theme::key_label_color(k),
                                    );
                                    if let Some(c) = full.analysis.key_confidence {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "confianza {:.0}%",
                                                c * 100.0
                                            ))
                                            .monospace()
                                            .color(t.cyan),
                                        );
                                        let bar = ui
                                            .allocate_exact_size(
                                                egui::vec2(120.0, 4.0),
                                                Sense::hover(),
                                            )
                                            .1
                                            .rect;
                                        ui.painter().rect_filled(bar, 2.0, t.container_high);
                                        let mut fill = bar;
                                        fill.set_width(120.0 * (c.clamp(0.0, 1.0) as f32));
                                        ui.painter().rect_filled(fill, 2.0, t.cyan);
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new("sin analizar").small().color(t.orange),
                                    );
                                }
                            });
                        // ENERGÍA & LOUDNESS
                        egui::Frame::none()
                            .fill(t.container)
                            .rounding(12.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_min_width(cw);
                                ui.label(card_title("ENERGÍA & LOUDNESS"));
                                ui.add_space(4.0);
                                if let Some(e) = full.analysis.energy {
                                    let (sw, sh, gap) = (7.0, 9.0, 2.0);
                                    let origin = ui.cursor().left_top();
                                    let val = (e.round().clamp(0.0, 10.0)) as i32;
                                    for s in 0..10 {
                                        let seg = egui::Rect::from_min_size(
                                            egui::pos2(origin.x + s as f32 * (sw + gap), origin.y),
                                            egui::vec2(sw, sh),
                                        );
                                        let col = if s < val {
                                            energy_color((s + 1) as f64)
                                        } else {
                                            t.container_high
                                        };
                                        ui.painter().rect_filled(seg, 1.0, col);
                                    }
                                    ui.add_space(14.0);
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · {} dB{}",
                                        full.analysis
                                            .lufs
                                            .map(|l| format!("{l:+.1} LUFS"))
                                            .unwrap_or_else(|| "—".into()),
                                        full.analysis
                                            .volume_db
                                            .map(|v| format!("{v:+.1}"))
                                            .unwrap_or_else(|| "—".into()),
                                        match full.analysis.clipped_peak_count.unwrap_or(0) {
                                            0 => String::new(),
                                            n => format!("  ⚠{n} clips"),
                                        }
                                    ))
                                    .monospace()
                                    .color(t.text_dim),
                                );
                            });
                    });
                    ui.add_space(8.0);
                    // CUE POINTS (§10: tarjeta ancha; origen EDITADO/AUTO §7)
                    egui::Frame::none()
                        .fill(t.container)
                        .rounding(12.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(card_title("CUE POINTS"));
                            ui.add_space(4.0);
                            let mut del: Option<usize> = None;
                            ui.horizontal(|ui| {
                                for (i, cue) in full.analysis.cues.iter().enumerate() {
                                    let edited = cue.origin == "user";
                                    // pad estilo CDJ-3000: letra A..H sobre el
                                    // color canónico de hot cue, borde accent si
                                    // es edición manual del usuario
                                    let pad = egui::Button::new(
                                        egui::RichText::new(
                                            crate::theme::hotcue_letter(i).to_string(),
                                        )
                                        .monospace()
                                        .strong()
                                        .color(t.bg),
                                    )
                                    .fill(crate::theme::hotcue_color(i))
                                    .min_size(egui::vec2(24.0, 24.0))
                                    .rounding(4.0)
                                    .stroke(if edited {
                                        egui::Stroke::new(1.5, t.accent)
                                    } else {
                                        egui::Stroke::NONE
                                    });
                                    if ui.add(pad).clicked() {
                                        if let Some(p) = deck_handles.as_ref() {
                                            let d = &p[self.active_deck];
                                            if !d.is_loaded() || d.path() != full.meta.path {
                                                d.load(&full.meta.path);
                                                self.deck_track_id[self.active_deck] = Some(id);
                                            }
                                            d.seek(cue.time_s);
                                            d.play();
                                        }
                                    }
                                    ui.label(
                                        egui::RichText::new(format!("{:>7.2}", cue.time_s))
                                            .monospace()
                                            .weak()
                                            .color(t.text_dim),
                                    );
                                    // borrado solo en modo EDIT (patrón rekordbox)
                                    if self.cues_editing
                                        && ui
                                            .small_button(
                                                egui::RichText::new("×").color(
                                                    egui::Color32::from_rgb(0xE5, 0x3E, 0x3E),
                                                ),
                                            )
                                            .clicked()
                                    {
                                        del = Some(i);
                                    }
                                }
                                if ui
                                    .small_button("+")
                                    .on_hover_text("cue en playhead (snap a beat)")
                                    .clicked()
                                {
                                    let mut cues2 = full.analysis.cues.clone();
                                    let tpos = self
                                        .player()
                                        .map(|p| p.decks[self.active_deck].position_s())
                                        .unwrap_or(0.0);
                                    let mut snapped = tpos;
                                    if let Some(bg) = &full.analysis.beatgrid {
                                        snapped = bg.beat_time(bg.beat_index_near(tpos));
                                    }
                                    cues2.push(CuePoint {
                                        time_s: snapped,
                                        energy_level: None,
                                        origin: "user".into(),
                                        label: None,
                                    });
                                    cues2.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
                                    if let Ok(mut db) = self.db.lock() {
                                        db.replace_cues(full.id, &cues2).ok();
                                    }
                                    self.load_detail(full.id);
                                }
                                // toggle EDIT (patrón rekordbox): muestra los ×
                                // de borrado junto a cada pad mientras esté activo
                                ui.separator();
                                let edit_btn = egui::Button::new(
                                    egui::RichText::new("EDIT")
                                        .monospace()
                                        .strong()
                                        .size(10.0)
                                        .color(if self.cues_editing { t.bg } else { t.text }),
                                )
                                .min_size(egui::vec2(38.0, 24.0))
                                .rounding(4.0);
                                let edit_btn = if self.cues_editing {
                                    edit_btn.fill(t.accent)
                                } else {
                                    edit_btn.fill(t.surface2)
                                };
                                if ui.add(edit_btn).clicked() {
                                    self.cues_editing = !self.cues_editing;
                                }
                            });
                            if let Some(i) = del {
                                let mut cues2 = full.analysis.cues.clone();
                                if cues2.len() > i {
                                    cues2.remove(i);
                                    if let Ok(mut db) = self.db.lock() {
                                        db.replace_cues(full.id, &cues2).ok();
                                    }
                                    self.load_detail(full.id);
                                }
                            }
                            ui.label(
                                egui::RichText::new(
                                    "EDITADO en lavanda · AUTO atenuado — clic salta al cue",
                                )
                                .small()
                                .color(t.text_dim),
                            );
                        });
                });
            });
    }

    pub(crate) fn load_detail(&mut self, id: i64) {
        if let Ok(db) = self.db.lock() {
            self.detail = db.track_full(id).unwrap_or(None);
            if let Some(cover) = db.cover(id).unwrap_or(None) {
                if let Some(img) = image::load_from_memory(&cover).ok() {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    self.cover_tex = Some(self.egui_ctx.load_texture(
                        format!("cover-{id}"),
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                        egui::TextureOptions::LINEAR,
                    ));
                    self.cover_tex_id = Some(id);
                    return;
                }
            }
        }
        self.cover_tex = None;
        self.cover_tex_id = None;
    }
}
