//! F-18/F-19: helpers puros de UI para playlists — aplanado del árbol de
//! playlists anidadas (colapso por carpeta, huérfanos/ciclos a salvo) y
//! paso de reordenación manual (F-19). Sin egui: testable directo.

use std::collections::HashMap;

use sm_model::Playlist;

/// Profundidad máxima del árbol de playlists (nivel 1..=3).
#[allow(dead_code)]
pub const MAX_DEPTH: u8 = 3;

/// Una fila del árbol lista para pintar en el sidebar.
#[derive(Debug, Clone)]
#[allow(dead_code)] // sustituido por flatten_playlists de studio_view
pub struct PlaylistRow {
    pub playlist: Playlist,
    /// 1 = raíz, 2 = sub, 3 = sub-sub...
    pub depth: u8,
    /// Tiene al menos una playlist hija.
    pub has_children: bool,
    /// Carpeta expandida (solo relevante si `has_children`).
    pub expanded: bool,
}

/// Aplana el árbol en orden de render: raíces por id, cada padre inmediato
/// antes de sus hijos (DFS), hijos por id. Las carpetas colapsadas ocultan
/// todo su subárbol.
///
/// Robusto ante datos corruptos: una playlist con padre inexistente
/// (huérfana) o atrapada en un ciclo de `parent_id` se pinta como raíz
/// (profundidad 1) — nunca se pierde ni cuelga el bucle.
#[allow(dead_code)] // la vista Studio aplana su propio arbol
pub fn flatten_tree(playlists: &[Playlist], collapsed: &HashMap<i64, bool>) -> Vec<PlaylistRow> {
    // hijos por padre en orden de id (la entrada viene ORDER BY id)
    let mut by_parent: HashMap<Option<i64>, Vec<&Playlist>> = HashMap::new();
    let ids: std::collections::HashSet<i64> = playlists.iter().map(|p| p.id).collect();
    for p in playlists {
        by_parent.entry(p.parent_id).or_default().push(p);
    }

    // raíz: parent None o padre inexistente (huérfana)
    let is_root = |p: &Playlist| match p.parent_id {
        None => true,
        Some(pid) => !ids.contains(&pid),
    };

    // pertenencia al árbol: alcanzable desde alguna raíz IGNORANDO el
    // colapso — separa "oculta por carpeta cerrada" de "fuera del árbol"
    let mut in_tree: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut walk: Vec<i64> = playlists
        .iter()
        .filter(|p| is_root(p))
        .map(|p| p.id)
        .collect();
    while let Some(id) = walk.pop() {
        if !in_tree.insert(id) {
            continue;
        }
        if let Some(children) = by_parent.get(&Some(id)) {
            for c in children {
                walk.push(c.id);
            }
        }
    }

    // render: DFS pre-orden respetando carpetas colapsadas
    let mut out: Vec<PlaylistRow> = Vec::new();
    let mut stack: Vec<(&Playlist, u8)> = Vec::new();
    for p in playlists.iter().filter(|p| is_root(p)).rev() {
        stack.push((p, 1));
    }
    while let Some((p, depth)) = stack.pop() {
        let children = by_parent.get(&Some(p.id)).cloned().unwrap_or_default();
        let has_children = !children.is_empty();
        let expanded = !collapsed.get(&p.id).copied().unwrap_or(false);
        out.push(PlaylistRow {
            playlist: p.clone(),
            depth,
            has_children,
            expanded,
        });
        if has_children && expanded {
            for c in children.iter().rev() {
                stack.push((c, depth + 1));
            }
        }
    }

    // nodos fuera del árbol (ciclos cerrados de parent_id): cada uno se
    // pinta como raíz plana en orden de id — sin recursión para no inventar
    // jerarquía sobre datos corruptos
    for p in playlists {
        if !in_tree.contains(&p.id) {
            out.push(PlaylistRow {
                playlist: p.clone(),
                depth: 1,
                has_children: false,
                expanded: true,
            });
        }
    }
    out
}

/// F-19: intercambia la posición `i` con su vecina inmediata
/// (`up = true` sube). Devuelve el nuevo orden completo, o `None` si `i`
/// está en el borde y no hay movimiento posible.
pub fn swap_step(ids: &[i64], i: usize, up: bool) -> Option<Vec<i64>> {
    let j = if up { i.checked_sub(1)? } else { i + 1 };
    if i >= ids.len() || j >= ids.len() {
        return None;
    }
    let mut out = ids.to_vec();
    out.swap(i, j);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(id: i64, name: &str, parent: Option<i64>) -> Playlist {
        Playlist {
            id,
            name: name.into(),
            kind: "manual".into(),
            smart_filter: None,
            parent_id: parent,
        }
    }

    fn names(rows: &[PlaylistRow]) -> Vec<(i64, u8)> {
        rows.iter().map(|r| (r.playlist.id, r.depth)).collect()
    }

    #[test]
    fn arbol_aplana_por_profundidad_y_orden_de_id() {
        // A(1) > B(2) > C(3); D(4) raíz independiente
        let pls = vec![
            pl(1, "A", None),
            pl(2, "B", Some(1)),
            pl(3, "C", Some(2)),
            pl(4, "D", None),
        ];
        let rows = flatten_tree(&pls, &HashMap::new());
        assert_eq!(
            names(&rows),
            vec![(1, 1), (2, 2), (3, 3), (4, 1)],
            "padre antes que hijos, hermanos por id"
        );
        let has: Vec<bool> = rows.iter().map(|r| r.has_children).collect();
        assert_eq!(has, vec![true, true, false, false]);
        assert!(rows.iter().all(|r| r.expanded), "expandidas por defecto");
    }

    #[test]
    fn carpeta_colapsada_oculta_su_subarbol() {
        let pls = vec![
            pl(1, "A", None),
            pl(2, "B", Some(1)),
            pl(3, "C", Some(2)),
            pl(4, "D", None),
        ];
        let mut collapsed = HashMap::new();
        collapsed.insert(1, true);
        let rows = flatten_tree(&pls, &collapsed);
        assert_eq!(
            names(&rows),
            vec![(1, 1), (4, 1)],
            "B y C ocultas bajo A colapsada"
        );
        // la fila A sigue marcada como carpeta, ahora contraída
        assert!(rows[0].has_children);
        assert!(!rows[0].expanded);

        // colapsar una hoja no rompe nada
        let mut collapsed2 = HashMap::new();
        collapsed2.insert(4, true);
        let rows2 = flatten_tree(&pls, &collapsed2);
        assert_eq!(names(&rows2), vec![(1, 1), (2, 2), (3, 3), (4, 1)]);
    }

    #[test]
    fn huerfanos_y_ciclos_se_pintan_como_raiz_sin_colgar() {
        // 5 huérfana (padre inexistente); 6>7>6 ciclo cerrado
        let pls = vec![
            pl(1, "A", None),
            pl(5, "H", Some(999)),
            pl(6, "X", Some(7)),
            pl(7, "Y", Some(6)),
        ];
        let rows = flatten_tree(&pls, &HashMap::new());
        assert_eq!(
            names(&rows),
            vec![(1, 1), (5, 1), (6, 1), (7, 1)],
            "huérfanos y nodos en ciclo cuelgan de raíz, en orden de id"
        );
    }

    #[test]
    fn profundidades_mas_profundas_que_max_sigue_aplanando() {
        // MAX_DEPTH limita la creación desde la UI, no el render:
        // una cadena de 4 (posible vía BD directa) se pinta completa
        let pls = vec![
            pl(1, "a", None),
            pl(2, "b", Some(1)),
            pl(3, "c", Some(2)),
            pl(4, "d", Some(3)),
        ];
        let rows = flatten_tree(&pls, &HashMap::new());
        assert_eq!(names(&rows), vec![(1, 1), (2, 2), (3, 3), (4, 4)]);
    }

    #[test]
    fn swap_step_mueve_una_posicion() {
        let ids = [10, 20, 30];
        assert_eq!(swap_step(&ids, 0, true), None, "primera no puede subir");
        assert_eq!(swap_step(&ids, 2, false), None, "última no puede bajar");
        assert_eq!(swap_step(&ids, 0, false), Some(vec![20, 10, 30]));
        assert_eq!(swap_step(&ids, 2, true), Some(vec![10, 30, 20]));
        assert_eq!(swap_step(&ids, 1, true), Some(vec![20, 10, 30]));
        assert_eq!(swap_step(&ids, 1, false), Some(vec![10, 30, 20]));
        // la entrada no muta
        assert_eq!(ids, [10, 20, 30]);
        // lista vacía / índice fuera de rango
        assert_eq!(swap_step(&[], 0, true), None);
        assert_eq!(swap_step(&ids, 9, true), None);
    }
}
