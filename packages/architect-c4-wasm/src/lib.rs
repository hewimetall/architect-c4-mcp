//! Browser WASM drawer: WebGPU detect → Canvas2D.
//! Zoom/pan re-render via ctx.setTransform (never CSS scale on the bitmap).

use architect_c4_scene::{
    collect_viewpoints, route_all_edges, EdgeRoute, SceneEdge, SceneGraph, SceneNode,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

fn layer_fill(layer: &str, group: bool) -> &'static str {
    if group {
        return match layer {
            "context" => "#dbeafe",
            "container" => "#e0f2fe",
            "component" => "#eef2ff",
            _ => "#f8fafc",
        };
    }
    match layer {
        "context" => "#1168BD",
        "container" => "#23A2D9",
        "component" => "#a5b4fc",
        "code" => "#ddd6fe",
        "external" => "#999999",
        _ => "#94a3b8",
    }
}

fn layer_stroke(layer: &str, group: bool) -> &'static str {
    if group {
        return match layer {
            "context" => "#1168BD",
            "container" => "#0284c7",
            "component" => "#6366f1",
            _ => "#94a3b8",
        };
    }
    "#0f172a33"
}

fn layer_text(layer: &str, group: bool) -> &'static str {
    if group {
        return "#0f172a";
    }
    match layer {
        "component" | "code" => "#1e1b4b",
        _ => "#ffffff",
    }
}

#[wasm_bindgen]
pub fn preferred_backend() -> String {
    let window = web_sys::window().expect("window");
    let nav = window.navigator();
    let gpu = js_sys::Reflect::get(nav.as_ref(), &JsValue::from_str("gpu")).ok();
    if gpu
        .as_ref()
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false)
    {
        "webgpu".into()
    } else {
        "canvas2d".into()
    }
}

fn dist_point_seg(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return (px - ax).hypot(py - ay);
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let qx = ax + t * dx;
    let qy = ay + t * dy;
    (px - qx).hypot(py - qy)
}

fn dist_point_poly(px: f64, py: f64, pts: &[(f64, f64)]) -> f64 {
    let mut best = f64::INFINITY;
    for w in pts.windows(2) {
        best = best.min(dist_point_seg(px, py, w[0].0, w[0].1, w[1].0, w[1].1));
    }
    best
}

fn label_chip_box(e: &SceneEdge, points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    if e.label.is_empty() || points.len() < 2 {
        return None;
    }
    let (lx, ly) = if e.label_x != 0.0 || e.label_y != 0.0 {
        (e.label_x, e.label_y)
    } else {
        let mid = &points[points.len() / 2];
        (mid.0 + 4.0, mid.1 - 16.0)
    };
    let lines: Vec<&str> = e.label.split('\n').collect();
    let mut max_w = 0.0_f64;
    for ln in &lines {
        max_w = max_w.max(ln.chars().count() as f64 * 5.6);
    }
    let tw = max_w + 12.0;
    let th = 12.0 * lines.len() as f64 + 4.0;
    // Match draw_label_chip rect: (lx-4, ly-11, tw, th)
    Some((lx - 4.0, ly - 11.0, tw, th))
}

fn point_in_chip(px: f64, py: f64, box_: (f64, f64, f64, f64)) -> bool {
    let (x, y, w, h) = box_;
    px >= x && px <= x + w && py >= y && py <= y + h
}

/// Hit-test relationship under a **world** (scene) point. Returns edge id or "".
/// Picks polyline within `hit_world` **or** note/label chip under the cursor.
#[wasm_bindgen]
pub fn hit_test_edge(scene_json: &str, world_x: f64, world_y: f64, hit_world: f64) -> String {
    let Ok(scene) = serde_json::from_str::<SceneGraph>(scene_json) else {
        return String::new();
    };
    let hit = hit_world.max(2.0);
    // Prefer note chip: hovering the caption highlights the whole relationship.
    for e in scene.edges.iter().rev() {
        if e.points.len() < 2 {
            continue;
        }
        if let Some(b) = label_chip_box(e, &e.points) {
            if point_in_chip(world_x, world_y, b) {
                return e.id.clone();
            }
        }
    }
    let mut best_id = String::new();
    let mut best_d = hit;
    for e in &scene.edges {
        if e.points.len() < 2 {
            continue;
        }
        let d = dist_point_poly(world_x, world_y, &e.points);
        if d <= best_d {
            best_d = d;
            best_id = e.id.clone();
        }
    }
    best_id
}

/// Hit-test a leaf/class node under a world point. Prefers deepest non-group.
#[wasm_bindgen]
pub fn hit_test_node(scene_json: &str, world_x: f64, world_y: f64) -> String {
    let Ok(scene) = serde_json::from_str::<SceneGraph>(scene_json) else {
        return String::new();
    };
    let mut best: Option<&SceneNode> = None;
    for n in &scene.nodes {
        if n.group {
            continue;
        }
        if world_x >= n.x
            && world_x <= n.x + n.w
            && world_y >= n.y
            && world_y <= n.y + n.h
            && best.map(|b| n.depth >= b.depth).unwrap_or(true)
        {
            best = Some(n);
        }
    }
    best.map(|n| n.id.clone()).unwrap_or_default()
}

/// Draw scene with camera (scale, pan) into a viewport-sized canvas.
///
/// Formulas (board-style, mouse-centered zoom done in JS before calling):
///   screen = world * scale + pan
///   canvas_buffer = css_size * devicePixelRatio
///   ctx.setTransform(dpr * scale, 0, 0, dpr * scale, dpr * pan_x, dpr * pan_y)
///
/// Never use CSS `transform: scale()` on the canvas — that upscales pixels ("шакалы").
/// `hover_edge_id` / `hover_node_id` — empty string = none.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn render_scene(
    canvas_id: &str,
    scene_json: &str,
    backend: &str,
    view_scale: f64,
    pan_x: f64,
    pan_y: f64,
    hover_edge_id: &str,
    hover_node_id: &str,
) -> Result<String, JsValue> {
    let scene: SceneGraph = serde_json::from_str(scene_json)
        .map_err(|e| JsValue::from_str(&format!("invalid scene json: {e}")))?;

    let mut used = backend.to_ascii_lowercase();
    if used == "auto" || used.is_empty() {
        used = preferred_backend();
    }
    if used == "webgpu" {
        if preferred_backend() != "webgpu" {
            log("architect-c4-wasm: WebGPU missing → Canvas2D");
        } else {
            log(
                "architect-c4-wasm: navigator.gpu present → Canvas2D redraw path (WebGPU mesh TBD)",
            );
        }
        used = "canvas2d".into();
    }

    // Allow contain-fit upscale (fill stage width); see docs/research/viewport-fit-resize.md
    let scale = view_scale.clamp(0.05, 16.0);
    let hover_e = if hover_edge_id.is_empty() {
        None
    } else {
        Some(hover_edge_id)
    };
    let hover_n = if hover_node_id.is_empty() {
        None
    } else {
        Some(hover_node_id)
    };
    draw_canvas2d(canvas_id, &scene, scale, pan_x, pan_y, hover_e, hover_n)?;
    Ok(used)
}

#[allow(clippy::too_many_arguments)]
fn draw_canvas2d(
    canvas_id: &str,
    scene: &SceneGraph,
    scale: f64,
    pan_x: f64,
    pan_y: f64,
    hover_edge_id: Option<&str>,
    hover_node_id: Option<&str>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let el = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas not found"))?;
    let canvas: HtmlCanvasElement = el.dyn_into()?;
    let dpr = window.device_pixel_ratio().max(1.0);

    // Viewport = CSS box of the canvas (fills the stage).
    // Headless / first-paint can report 0 — fall back to parent or window.
    let mut css_w = canvas.client_width() as f64;
    let mut css_h = canvas.client_height() as f64;
    if css_w < 2.0 || css_h < 2.0 {
        if let Some(parent) = canvas.parent_element() {
            css_w = parent.client_width() as f64;
            css_h = parent.client_height() as f64;
        }
    }
    if css_w < 2.0 || css_h < 2.0 {
        css_w = window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1280.0);
        css_h = window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(800.0)
            - 56.0;
    }
    let css_w = css_w.max(1.0);
    let css_h = css_h.max(1.0);
    let buf_w = (css_w * dpr).round().max(1.0) as u32;
    let buf_h = (css_h * dpr).round().max(1.0) as u32;
    if canvas.width() != buf_w {
        canvas.set_width(buf_w);
    }
    if canvas.height() != buf_h {
        canvas.set_height(buf_h);
    }

    let ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("2d context"))?
        .dyn_into::<CanvasRenderingContext2d>()?;

    // Clear in device pixels.
    ctx.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)?;
    ctx.set_fill_style_str("#f8fafc");
    ctx.fill_rect(0.0, 0.0, buf_w as f64, buf_h as f64);

    // Camera: CSS pixels → device pixels, then world → screen.
    // setTransform(a, b, c, d, e, f): x' = a*x + e, y' = d*y + f
    ctx.set_transform(dpr * scale, 0.0, 0.0, dpr * scale, dpr * pan_x, dpr * pan_y)?;

    // Optional light world backdrop matching scene size
    ctx.set_fill_style_str("#ffffff");
    ctx.fill_rect(0.0, 0.0, scene.width.max(1.0), scene.height.max(1.0));

    // Keep strokes ~1 CSS px on screen regardless of zoom.
    let inv = 1.0 / scale;
    // Prefer matryoshka polylines/ports baked into the scene; legacy fallback otherwise.
    let use_scene_routes = scene.edges.iter().any(|e| e.points.len() >= 2);
    let legacy_routes: Vec<EdgeRoute> = if use_scene_routes {
        Vec::new()
    } else {
        route_all_edges(&scene.nodes, &scene.edges)
    };

    let mut groups: Vec<&SceneNode> = scene.nodes.iter().filter(|n| n.group).collect();
    groups.sort_by_key(|a| a.depth);
    let leaves: Vec<&SceneNode> = scene.nodes.iter().filter(|n| !n.group).collect();

    let hover_e = hover_edge_id.unwrap_or("");
    let hover_n = hover_node_id.unwrap_or("");
    let hovering = !hover_e.is_empty() || !hover_n.is_empty();

    for n in &groups {
        draw_node(&ctx, n, inv, false)?;
    }

    // Connected edges when a class is hovered (each line separate — never glued).
    let node_edge_ids: Vec<&str> = if !hover_n.is_empty() {
        scene
            .edges
            .iter()
            .filter(|e| e.from == hover_n || e.to == hover_n)
            .map(|e| e.id.as_str())
            .collect()
    } else {
        Vec::new()
    };

    for (i, e) in scene.edges.iter().enumerate() {
        let is_focus = e.id == hover_e || node_edge_ids.iter().any(|id| *id == e.id);
        if is_focus {
            continue; // draw focused edges last
        }
        let points: &[(f64, f64)] = if e.points.len() >= 2 {
            &e.points
        } else if let Some(r) = legacy_routes.get(i) {
            &r.points
        } else {
            continue;
        };
        if points.len() < 2 {
            continue;
        }
        let alpha = if hovering { 0.28 } else { 1.0 };
        draw_edge_polyline(&ctx, e, points, inv, false, alpha)?;
    }

    // Endpoint / class glow under leaves.
    if !hover_e.is_empty() {
        if let Some(he) = scene.edges.iter().find(|e| e.id == hover_e) {
            for nid in [&he.from, &he.to] {
                if let Some(n) = scene.nodes.iter().find(|n| n.id == *nid) {
                    ctx.set_stroke_style_str("#f59e0b");
                    ctx.set_line_width(3.0 * inv);
                    ctx.stroke_rect(n.x - 2.0, n.y - 2.0, n.w + 4.0, n.h + 4.0);
                }
            }
        }
    }

    for n in &leaves {
        let hi = !hover_n.is_empty() && n.id == hover_n;
        draw_node(&ctx, n, inv, hi)?;
    }

    // Focused edges on top (per-edge, never merged).
    for (i, e) in scene.edges.iter().enumerate() {
        let is_focus = e.id == hover_e || node_edge_ids.iter().any(|id| *id == e.id);
        if !is_focus {
            continue;
        }
        let points: &[(f64, f64)] = if e.points.len() >= 2 {
            &e.points
        } else if let Some(r) = legacy_routes.get(i) {
            &r.points
        } else {
            continue;
        };
        if points.len() >= 2 {
            draw_edge_polyline(&ctx, e, points, inv, true, 1.0)?;
        }
    }

    let vp_size = 2.4 * inv;
    if !scene.ports.is_empty() {
        for p in &scene.ports {
            draw_viewpoint_rhombus(&ctx, p.x, p.y, vp_size, inv)?;
        }
    } else {
        let viewpoints = collect_viewpoints(&legacy_routes);
        for vp in &viewpoints {
            draw_viewpoint_rhombus(&ctx, vp.x, vp.y, vp_size, inv)?;
        }
    }

    // Notes on TOP of everything (above leaves/groups in paint order).
    // Draw non-hovered first, hovered chip last (on top + stronger accent).
    let mut hover_chip: Option<(&str, f64, f64, f64)> = None;
    for (i, e) in scene.edges.iter().enumerate() {
        let points: &[(f64, f64)] = if e.points.len() >= 2 {
            &e.points
        } else if let Some(r) = legacy_routes.get(i) {
            &r.points
        } else {
            continue;
        };
        if points.len() < 2 || e.label.is_empty() {
            continue;
        }
        let (lx, ly) = if e.label_x != 0.0 || e.label_y != 0.0 {
            (e.label_x, e.label_y)
        } else {
            let mid = &points[points.len() / 2];
            (mid.0 + 4.0, mid.1 - 16.0)
        };
        let hi = e.id == hover_e || node_edge_ids.iter().any(|id| *id == e.id);
        if hi {
            hover_chip = Some((e.label.as_str(), lx, ly, inv));
        } else {
            draw_label_chip(&ctx, &e.label, lx, ly, inv, false, hovering);
        }
    }
    if let Some((label, lx, ly, inv)) = hover_chip {
        draw_label_chip(&ctx, label, lx, ly, inv, true, false);
    }
    Ok(())
}

fn kind_stroke(ek: &str) -> &'static str {
    match ek {
        "implements" => "#7c3aed",  // violet dashed
        "extends" => "#1d4ed8",     // blue solid + hollow △
        "composition" => "#b45309", // filled diamond
        "aggregation" => "#0f766e", // hollow diamond
        _ => "#475569",             // assoc
    }
}

fn draw_edge_polyline(
    ctx: &CanvasRenderingContext2d,
    e: &SceneEdge,
    points: &[(f64, f64)],
    inv: f64,
    highlight: bool,
    alpha: f64,
) -> Result<(), JsValue> {
    let ek = e.edge_kind.as_str();
    let base = kind_stroke(ek);
    let (stroke, width) = if highlight {
        ("#f59e0b", 3.6 * inv)
    } else {
        let w = if points.len() > 4 {
            2.0 * inv
        } else {
            1.5 * inv
        };
        (base, w)
    };
    if highlight {
        ctx.set_stroke_style_str("rgba(245,158,11,0.28)");
        ctx.set_line_width(width * 2.4);
        ctx.set_line_dash(&js_sys::Array::new())?;
        ctx.begin_path();
        ctx.move_to(points[0].0, points[0].1);
        for p in points.iter().skip(1) {
            ctx.line_to(p.0, p.1);
        }
        ctx.stroke();
    }
    if alpha < 0.99 && !highlight {
        ctx.set_global_alpha(alpha);
    }
    ctx.set_stroke_style_str(stroke);
    ctx.set_line_width(width);
    // Connectivity type → stroke style (kept even when highlighted for dash of implements).
    if ek == "implements" {
        ctx.set_line_dash(&js_sys::Array::of2(
            &JsValue::from_f64(7.0 * inv),
            &JsValue::from_f64(4.0 * inv),
        ))?;
    } else {
        ctx.set_line_dash(&js_sys::Array::new())?;
    }
    ctx.begin_path();
    ctx.move_to(points[0].0, points[0].1);
    for p in points.iter().skip(1) {
        ctx.line_to(p.0, p.1);
    }
    ctx.stroke();
    ctx.set_line_dash(&js_sys::Array::new())?;
    if let (Some(a), Some(b)) = (points.iter().rev().nth(1), points.last()) {
        match ek {
            "extends" | "implements" => {
                draw_uml_generalization(ctx, a.0, a.1, b.0, b.1, 11.0 * inv.max(0.5), stroke);
            }
            "composition" => {
                draw_uml_diamond(ctx, a.0, a.1, b.0, b.1, 10.0 * inv.max(0.5), stroke, true);
            }
            "aggregation" => {
                draw_uml_diamond(ctx, a.0, a.1, b.0, b.1, 10.0 * inv.max(0.5), stroke, false);
            }
            _ => {
                draw_arrowhead(ctx, a.0, a.1, b.0, b.1, 9.0 * inv.max(0.5), stroke);
            }
        }
    }
    if matches!(ek, "assoc" | "") && points.len() >= 2 {
        draw_source_tick(
            ctx,
            points[0].0,
            points[0].1,
            points[1].0,
            points[1].1,
            5.0 * inv.max(0.5),
            stroke,
        );
    }
    ctx.set_global_alpha(1.0);
    Ok(())
}

fn draw_label_chip(
    ctx: &CanvasRenderingContext2d,
    label: &str,
    lx: f64,
    ly: f64,
    inv: f64,
    highlight: bool,
    dim: bool,
) {
    let lines: Vec<&str> = label.split('\n').collect();
    ctx.set_font("10px ui-sans-serif, system-ui, sans-serif");
    let mut max_w = 0.0_f64;
    for ln in &lines {
        max_w = max_w.max(ln.chars().count() as f64 * 5.6);
    }
    let tw = max_w + 12.0;
    let th = 12.0 * lines.len() as f64 + 4.0;
    let x0 = lx - 4.0;
    let y0 = ly - 11.0;
    if dim {
        ctx.set_global_alpha(0.4);
    }
    if highlight {
        // Outer glow
        ctx.set_fill_style_str("rgba(245,158,11,0.22)");
        ctx.fill_rect(x0 - 3.0, y0 - 3.0, tw + 6.0, th + 6.0);
        ctx.set_fill_style_str("rgba(255,251,235,0.98)");
        ctx.set_stroke_style_str("#f59e0b");
        ctx.set_line_width(2.2 * inv);
    } else {
        ctx.set_fill_style_str("rgba(248,250,252,0.96)");
        ctx.set_stroke_style_str("rgba(15,23,42,0.12)");
        ctx.set_line_width(1.0 * inv);
    }
    ctx.fill_rect(x0, y0, tw, th);
    ctx.stroke_rect(x0, y0, tw, th);
    ctx.set_fill_style_str(if highlight { "#92400e" } else { "#0f172a" });
    for (i, ln) in lines.iter().enumerate() {
        let _ = ctx.fill_text(ln, lx, ly + i as f64 * 12.0);
    }
    ctx.set_global_alpha(1.0);
}

fn draw_node(
    ctx: &CanvasRenderingContext2d,
    n: &SceneNode,
    inv: f64,
    highlight: bool,
) -> Result<(), JsValue> {
    // Code leaves = UML class boxes (parity with Mermaid classDiagram / ADR 0004).
    if !n.group && (n.kind == "code" || n.layer == "code") {
        return draw_uml_class(ctx, n, inv, highlight);
    }
    let fill = layer_fill(&n.layer, n.group);
    let stroke = if highlight {
        "#f59e0b"
    } else {
        layer_stroke(&n.layer, n.group)
    };
    let text = layer_text(&n.layer, n.group);
    if highlight {
        ctx.set_stroke_style_str("rgba(245,158,11,0.35)");
        ctx.set_line_width(6.0 * inv);
        round_rect(ctx, n.x - 3.0, n.y - 3.0, n.w + 6.0, n.h + 6.0, 12.0);
        ctx.stroke();
    }
    round_rect(ctx, n.x, n.y, n.w, n.h, if n.group { 14.0 } else { 10.0 });
    ctx.set_fill_style_str(fill);
    ctx.fill();
    ctx.set_stroke_style_str(stroke);
    ctx.set_line_width(if highlight {
        2.5 * inv
    } else if n.group {
        2.0 * inv
    } else {
        1.0 * inv
    });
    if n.group {
        ctx.set_line_dash(&js_sys::Array::of2(
            &JsValue::from_f64(6.0 * inv),
            &JsValue::from_f64(4.0 * inv),
        ))?;
    } else {
        ctx.set_line_dash(&js_sys::Array::new())?;
    }
    ctx.stroke();
    ctx.set_line_dash(&js_sys::Array::new())?;

    ctx.set_fill_style_str(text);
    ctx.set_font("bold 13px ui-sans-serif, system-ui, sans-serif");
    let title = if n.group {
        format!("[{}] {}", n.layer, n.name)
    } else {
        n.name.clone()
    };
    let _ = ctx.fill_text(&title, n.x + 12.0, n.y + 26.0);
    if !n.group {
        ctx.set_font("11px ui-sans-serif, system-ui, sans-serif");
        ctx.set_global_alpha(0.85);
        let _ = ctx.fill_text(&n.kind, n.x + 12.0, n.y + 46.0);
        ctx.set_global_alpha(1.0);
    }
    Ok(())
}

/// UML class: stereotype + name compartment + members compartment.
fn draw_uml_class(
    ctx: &CanvasRenderingContext2d,
    n: &SceneNode,
    inv: f64,
    highlight: bool,
) -> Result<(), JsValue> {
    let fill = if highlight { "#fffbeb" } else { "#f5f3ff" };
    let stroke = if highlight { "#f59e0b" } else { "#5b21b6" };
    if highlight {
        ctx.set_stroke_style_str("rgba(245,158,11,0.4)");
        ctx.set_line_width(7.0 * inv);
        round_rect(ctx, n.x - 4.0, n.y - 4.0, n.w + 8.0, n.h + 8.0, 6.0);
        ctx.stroke();
    }
    // Sharp-ish corners like classic UML class boxes
    round_rect(ctx, n.x, n.y, n.w, n.h, 4.0);
    ctx.set_fill_style_str(fill);
    ctx.fill();
    ctx.set_stroke_style_str(stroke);
    ctx.set_line_width(if highlight { 2.4 * inv } else { 1.25 * inv });
    ctx.set_line_dash(&js_sys::Array::new())?;
    ctx.stroke();

    let mut y = n.y + 16.0;
    if let Some(st) = n.stereotype.as_deref() {
        ctx.set_fill_style_str("#6d28d9");
        ctx.set_font("italic 11px ui-sans-serif, system-ui, sans-serif");
        let _ = ctx.fill_text(&format!("«{st}»"), n.x + 10.0, y);
        y += 14.0;
    }
    ctx.set_fill_style_str("#1e1b4b");
    ctx.set_font("bold 13px ui-sans-serif, system-ui, sans-serif");
    let _ = ctx.fill_text(&n.name, n.x + 10.0, y);
    y += 10.0;
    // Divider under name
    ctx.set_stroke_style_str("#c4b5fd");
    ctx.set_line_width(1.0 * inv);
    ctx.begin_path();
    ctx.move_to(n.x + 4.0, y);
    ctx.line_to(n.x + n.w - 4.0, y);
    ctx.stroke();
    y += 14.0;

    ctx.set_fill_style_str("#312e81");
    ctx.set_font("11px ui-monospace, SFMono-Regular, Menlo, monospace");
    if n.members.is_empty() {
        ctx.set_global_alpha(0.55);
        let _ = ctx.fill_text("+…()", n.x + 10.0, y);
        ctx.set_global_alpha(1.0);
    } else {
        let max_chars = ((n.w - 20.0) / 6.5).floor().max(12.0) as usize;
        for m in &n.members {
            let shown = if m.chars().count() > max_chars {
                let mut s: String = m.chars().take(max_chars.saturating_sub(1)).collect();
                s.push('…');
                s
            } else {
                m.clone()
            };
            let _ = ctx.fill_text(&shown, n.x + 10.0, y);
            y += 16.0;
            if y > n.y + n.h - 8.0 {
                break;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_uml_diamond(
    ctx: &CanvasRenderingContext2d,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    size: f64,
    color: &str,
    filled: bool,
) {
    let angle = (y1 - y0).atan2(x1 - x0);
    let dx = angle.cos() * size;
    let dy = angle.sin() * size;
    let px = -angle.sin() * size * 0.55;
    let py = angle.cos() * size * 0.55;
    let tip_x = x1;
    let tip_y = y1;
    let mid_x = x1 - dx;
    let mid_y = y1 - dy;
    let back_x = x1 - dx * 2.0;
    let back_y = y1 - dy * 2.0;
    ctx.set_stroke_style_str(color);
    ctx.set_fill_style_str(if filled { color } else { "#ffffff" });
    ctx.set_line_width(1.25);
    ctx.begin_path();
    ctx.move_to(tip_x, tip_y);
    ctx.line_to(mid_x + px, mid_y + py);
    ctx.line_to(back_x, back_y);
    ctx.line_to(mid_x - px, mid_y - py);
    ctx.close_path();
    ctx.fill();
    ctx.stroke();
}

fn draw_uml_generalization(
    ctx: &CanvasRenderingContext2d,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    size: f64,
    color: &str,
) {
    let angle = (y1 - y0).atan2(x1 - x0);
    let ax = x1 - size * angle.cos();
    let ay = y1 - size * angle.sin();
    ctx.set_stroke_style_str(color);
    ctx.set_fill_style_str("#ffffff");
    ctx.set_line_width(1.2);
    ctx.begin_path();
    ctx.move_to(x1, y1);
    ctx.line_to(
        ax - size * 0.55 * (angle - 1.2).cos(),
        ay - size * 0.55 * (angle - 1.2).sin(),
    );
    ctx.line_to(
        ax - size * 0.55 * (angle + 1.2).cos(),
        ay - size * 0.55 * (angle + 1.2).sin(),
    );
    ctx.close_path();
    ctx.fill();
    ctx.stroke();
}

/// Small rhombus marker at a border viewpoint (arrow dock).
fn draw_viewpoint_rhombus(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    size: f64,
    inv: f64,
) -> Result<(), JsValue> {
    ctx.begin_path();
    ctx.move_to(x, y - size);
    ctx.line_to(x + size, y);
    ctx.line_to(x, y + size);
    ctx.line_to(x - size, y);
    ctx.close_path();
    ctx.set_fill_style_str("#ffffff");
    ctx.fill();
    ctx.set_stroke_style_str("#312e81");
    ctx.set_line_width(1.0 * inv);
    ctx.set_line_dash(&js_sys::Array::new())?;
    ctx.stroke();
    Ok(())
}

fn draw_arrowhead(
    ctx: &CanvasRenderingContext2d,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    size: f64,
    color: &str,
) {
    let angle = (y1 - y0).atan2(x1 - x0);
    ctx.set_fill_style_str(color);
    ctx.begin_path();
    ctx.move_to(x1, y1);
    ctx.line_to(
        x1 - size * (angle - 0.4).cos(),
        y1 - size * (angle - 0.4).sin(),
    );
    ctx.line_to(
        x1 - size * (angle + 0.4).cos(),
        y1 - size * (angle + 0.4).sin(),
    );
    ctx.close_path();
    ctx.fill();
}

/// Small bar at the source port — reads as "starts here".
fn draw_source_tick(
    ctx: &CanvasRenderingContext2d,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    size: f64,
    color: &str,
) {
    let angle = (y1 - y0).atan2(x1 - x0);
    let px = -angle.sin();
    let py = angle.cos();
    ctx.set_stroke_style_str(color);
    ctx.set_line_width(size * 0.45);
    ctx.begin_path();
    ctx.move_to(x0 - px * size, y0 - py * size);
    ctx.line_to(x0 + px * size, y0 + py * size);
    ctx.stroke();
}

fn round_rect(ctx: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, r: f64) {
    ctx.begin_path();
    ctx.move_to(x + r, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
    let _ = ctx.arc_to(x, y + h, x, y, r);
    let _ = ctx.arc_to(x, y, x + w, y, r);
    ctx.close_path();
}

#[cfg(test)]
mod tests {
    #[test]
    fn camera_scale_clamp_bounds() {
        let s = 20.0_f64.clamp(0.05, 16.0);
        assert!((s - 16.0).abs() < f64::EPSILON);
        let s2 = 0.01_f64.clamp(0.05, 16.0);
        assert!((s2 - 0.05).abs() < f64::EPSILON);
    }
}
