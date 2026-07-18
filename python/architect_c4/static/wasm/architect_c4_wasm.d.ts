/* tslint:disable */
/* eslint-disable */

/**
 * Hit-test relationship under a **world** (scene) point. Returns edge id or "".
 * Picks polyline within `hit_world` **or** note/label chip under the cursor.
 */
export function hit_test_edge(scene_json: string, world_x: number, world_y: number, hit_world: number): string;

/**
 * Hit-test a leaf/class node under a world point. Prefers deepest non-group.
 */
export function hit_test_node(scene_json: string, world_x: number, world_y: number): string;

export function preferred_backend(): string;

/**
 * Draw scene with camera (scale, pan) into a viewport-sized canvas.
 *
 * Formulas (board-style, mouse-centered zoom done in JS before calling):
 *   screen = world * scale + pan
 *   canvas_buffer = css_size * devicePixelRatio
 *   ctx.setTransform(dpr * scale, 0, 0, dpr * scale, dpr * pan_x, dpr * pan_y)
 *
 * Never use CSS `transform: scale()` on the canvas — that upscales pixels ("шакалы").
 * `hover_edge_id` / `hover_node_id` — empty string = none.
 */
export function render_scene(canvas_id: string, scene_json: string, backend: string, view_scale: number, pan_x: number, pan_y: number, hover_edge_id: string, hover_node_id: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly hit_test_edge: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly hit_test_node: (a: number, b: number, c: number, d: number) => [number, number];
    readonly preferred_backend: () => [number, number];
    readonly render_scene: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
