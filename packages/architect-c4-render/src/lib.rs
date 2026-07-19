//! C4 diagrams via Mermaid C4Context/C4Container/C4Component + readable HTML viewer.
//! Visual language follows Simon Brown C4 colors (person / system / container / component).

use architect_c4_domain::element_uml_members;
use architect_c4_domain::{C4Layer, Decision, Element, ElementKind, Flow, FlowKind, Relationship};
use architect_c4_scene::{build_scene, scene_to_json, SceneInput, ViewMode};

#[derive(Debug, Clone)]
pub struct DiagramInput<'a> {
    pub elements: &'a [Element],
    pub relationships: &'a [Relationship],
    pub base_url: &'a str,
}

pub fn overview_mermaid(input: &DiagramInput<'_>) -> String {
    diagram_for_layer(input, C4Layer::Context, None)
}

pub fn diagram_for_layer(
    input: &DiagramInput<'_>,
    layer: C4Layer,
    parent_id: Option<&str>,
) -> String {
    match layer {
        C4Layer::Landscape | C4Layer::Context => c4_context(input),
        C4Layer::Container => c4_container(input, parent_id),
        C4Layer::Component => c4_component(input, parent_id),
        C4Layer::Code => code_class_diagram(input, parent_id),
        C4Layer::Adr => {
            String::from("flowchart TB\n  empty[\"ADR layer - use list_adrs or /adrs\"]\n")
        }
    }
}

/// Nested flowchart with system/container subgraphs (C4-style grouping).
pub fn all_layers_mermaid(input: &DiagramInput<'_>, focus: Option<&str>) -> String {
    let mut out = String::from("flowchart TB\n");
    out.push_str("  classDef person fill:#08427B,color:#fff,stroke:#052E56\n");
    out.push_str("  classDef system fill:#1168BD,color:#fff,stroke:#0B4884\n");
    out.push_str("  classDef external fill:#999999,color:#fff,stroke:#6b7280\n");
    out.push_str("  classDef container fill:#23A2D9,color:#fff,stroke:#0B4884\n");
    out.push_str("  classDef component fill:#a5b4fc,color:#1e1b4b,stroke:#6366f1\n");
    out.push_str("  classDef code fill:#ddd6fe,color:#1e1b4b,stroke:#6d28d9\n");

    let els: Vec<&Element> = if let Some(f) = focus {
        input
            .elements
            .iter()
            .filter(|e| {
                e.id == f
                    || e.parent_id.as_deref() == Some(f)
                    || ancestor_of(input.elements, f, &e.id)
                    || descendant_of(input.elements, &e.id, f)
            })
            .collect()
    } else {
        input.elements.iter().collect()
    };

    // Persons + externals outside (or beside) systems
    for e in els.iter().filter(|e| e.kind == ElementKind::Person) {
        out.push_str(&format!(
            "  {}[\"{}\"]:::person\n",
            sanitize_alias(&e.id),
            c4_label(&e.name)
        ));
    }
    for e in els.iter().filter(|e| e.kind == ElementKind::External) {
        let tech = e
            .technology
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("external");
        out.push_str(&format!(
            "  {}[\"{}\\n[{}]\"]:::external\n",
            sanitize_alias(&e.id),
            c4_label(&e.name),
            c4_label(tech)
        ));
    }

    let systems: Vec<&Element> = els
        .iter()
        .copied()
        .filter(|e| e.kind == ElementKind::SoftwareSystem)
        .collect();
    for sys in &systems {
        emit_system_subgraph(&mut out, &els, sys, 1);
    }
    // Orphan containers (focused without system in set)
    for c in els.iter().filter(|e| {
        e.kind == ElementKind::Container
            && !systems
                .iter()
                .any(|s| e.parent_id.as_deref() == Some(s.id.as_str()))
    }) {
        emit_container_subgraph(&mut out, &els, c, 1);
    }

    let ids: std::collections::HashSet<&str> = els.iter().map(|e| e.id.as_str()).collect();
    for r in input.relationships {
        if ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str()) {
            // Quoted label avoids Mermaid mistaking `*` / arrows for markdown.
            out.push_str(&format!(
                "  {} -->|\"{}\"| {}\n",
                sanitize_alias(&r.from_id),
                c4_rel_label(r.description.as_deref()),
                sanitize_alias(&r.to_id)
            ));
        }
    }
    if els.is_empty() {
        out.push_str("  empty[\"No elements\"]\n");
    }
    out
}

fn descendant_of(elements: &[Element], id: &str, ancestor: &str) -> bool {
    let mut cur = elements.iter().find(|e| e.id == id);
    while let Some(e) = cur {
        if e.parent_id.as_deref() == Some(ancestor) {
            return true;
        }
        cur = e
            .parent_id
            .as_deref()
            .and_then(|p| elements.iter().find(|x| x.id == p));
    }
    false
}

fn ancestor_of(elements: &[Element], id: &str, maybe_descendant: &str) -> bool {
    descendant_of(elements, maybe_descendant, id)
}

fn emit_system_subgraph(out: &mut String, els: &[&Element], sys: &Element, indent: usize) {
    let pad = "  ".repeat(indent);
    let sid = sanitize_alias(&sys.id);
    let containers: Vec<&Element> = els
        .iter()
        .copied()
        .filter(|e| {
            e.kind == ElementKind::Container && e.parent_id.as_deref() == Some(sys.id.as_str())
        })
        .collect();
    // Leaf system → plain node. Nested → boundary without duplicate system node.
    if containers.is_empty() {
        out.push_str(&format!(
            "{pad}{sid}[\"{}\"]:::system\n",
            c4_label(&sys.name)
        ));
        return;
    }
    out.push_str(&format!(
        "{pad}subgraph {sid}[\"{}\"]\n{pad}  direction LR\n",
        c4_label(&sys.name)
    ));
    for c in &containers {
        emit_container_subgraph(out, els, c, indent + 1);
    }
    out.push_str(&format!("{pad}end\n"));
    out.push_str(&format!(
        "{pad}style {sid} fill:#dbeafe,stroke:#1168BD,stroke-width:2px,color:#0f172a\n"
    ));
}

fn emit_container_subgraph(out: &mut String, els: &[&Element], c: &Element, indent: usize) {
    let pad = "  ".repeat(indent);
    let cid = sanitize_alias(&c.id);
    let comps: Vec<&Element> = els
        .iter()
        .copied()
        .filter(|e| {
            e.kind == ElementKind::Component && e.parent_id.as_deref() == Some(c.id.as_str())
        })
        .collect();
    let title = match c
        .technology
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(tech) => format!("{}\\n[{}]", c.name, tech),
        None => c.name.clone(),
    };
    // Leaf container → node; nested → titled boundary.
    if comps.is_empty() {
        out.push_str(&format!(
            "{pad}{cid}[\"{}\"]:::container\n",
            c4_label(&title)
        ));
        return;
    }
    out.push_str(&format!(
        "{pad}subgraph {cid}[\"{}\"]\n{pad}  direction TB\n",
        c4_label(&title)
    ));
    for comp in &comps {
        let comp_id = sanitize_alias(&comp.id);
        let codes: Vec<&Element> = els
            .iter()
            .copied()
            .filter(|e| {
                e.kind == ElementKind::Code && e.parent_id.as_deref() == Some(comp.id.as_str())
            })
            .collect();
        if codes.is_empty() {
            out.push_str(&format!(
                "{pad}  {comp_id}[\"{}\"]:::component\n",
                c4_label(&comp.name)
            ));
        } else {
            out.push_str(&format!(
                "{pad}  subgraph {comp_id}[\"{}\"]\n{pad}    direction LR\n",
                c4_label(&comp.name)
            ));
            for code in &codes {
                out.push_str(&format!(
                    "{pad}    {}[\"{}\"]:::code\n",
                    sanitize_alias(&code.id),
                    c4_label(&code.name)
                ));
            }
            out.push_str(&format!("{pad}  end\n"));
            out.push_str(&format!(
                "{pad}  style {comp_id} fill:#eef2ff,stroke:#6366f1,stroke-width:1.5px,color:#1e1b4b\n"
            ));
        }
    }
    out.push_str(&format!("{pad}end\n"));
    out.push_str(&format!(
        "{pad}style {cid} fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#0f172a\n"
    ));
}

pub fn scene_json_for_view(
    elements: &[Element],
    relationships: &[Relationship],
    mode: ViewMode,
    layer: Option<&str>,
    focus: Option<&str>,
) -> String {
    scene_to_json(&build_scene(&SceneInput {
        elements,
        relationships,
        mode,
        layer,
        focus,
    }))
}

/// Legacy helper used by older tests.
pub fn layer_mermaid(
    input: &DiagramInput<'_>,
    parent_id: Option<&str>,
    kinds: &[ElementKind],
) -> String {
    let layer = kinds
        .first()
        .map(|k| k.layer())
        .unwrap_or(C4Layer::Container);
    diagram_for_layer(input, layer, parent_id)
}

fn c4_context(input: &DiagramInput<'_>) -> String {
    let mut out = String::from("C4Context\n");
    out.push_str("  title System Context\n");
    let mut ids = std::collections::HashSet::new();
    for e in input.elements {
        match e.kind {
            ElementKind::Person => {
                ids.insert(e.id.as_str());
                out.push_str(&format!(
                    "  Person({}, \"{}\", \"{}\")\n",
                    sanitize_alias(&e.id),
                    c4_label(&e.name),
                    c4_label(e.description.as_deref().unwrap_or(e.kind.as_str()))
                ));
            }
            ElementKind::External => {
                ids.insert(e.id.as_str());
                let desc = e.description.as_deref().unwrap_or("external");
                out.push_str(&format!(
                    "  System_Ext({}, \"{}\", \"{}\")\n",
                    sanitize_alias(&e.id),
                    c4_label(&e.name),
                    c4_label(desc)
                ));
            }
            ElementKind::SoftwareSystem => {
                ids.insert(e.id.as_str());
                // Marker "external" (prefix/word) ⇒ System_Ext, but never show the marker in the box.
                let (external, desc) = external_desc(e.description.as_deref());
                if external {
                    out.push_str(&format!(
                        "  System_Ext({}, \"{}\", \"{}\")\n",
                        sanitize_alias(&e.id),
                        c4_label(&e.name),
                        c4_label(&desc)
                    ));
                } else {
                    out.push_str(&format!(
                        "  System({}, \"{}\", \"{}\")\n",
                        sanitize_alias(&e.id),
                        c4_label(&e.name),
                        c4_label(&desc)
                    ));
                }
            }
            _ => {}
        }
    }
    if ids.is_empty() {
        return String::from(
            "C4Context\n  title System Context\n  System(empty, \"(no elements)\", \"\")\n",
        );
    }
    emit_rels(&mut out, input, &ids);
    // Fewer shapes per row = less Rel-label pileup (mermaid-studio / C4 layout tip).
    out.push_str("  UpdateLayoutConfig($c4ShapeInRow=\"2\", $c4BoundaryInRow=\"1\")\n");
    out
}

fn c4_container(input: &DiagramInput<'_>, parent_id: Option<&str>) -> String {
    let Some(parent) = parent_id else {
        return String::from(
            "C4Container\n  title Containers\n  System(empty, \"Pick a software system parent\", \"n/a\")\n",
        );
    };
    let parent_el = input.elements.iter().find(|e| e.id == parent);
    let title = parent_el
        .map(|e| format!("Containers - {}", e.name))
        .unwrap_or_else(|| "Containers".into());

    let mut out = String::from("C4Container\n");
    out.push_str(&format!("  title {}\n", c4_label(&title)));

    // Boundary for the parent system
    out.push_str(&format!(
        "  System_Boundary({}, \"{}\") {{\n",
        sanitize_alias(&format!("{parent}_b")),
        c4_label(parent_el.map(|e| e.name.as_str()).unwrap_or(parent))
    ));

    let mut ids = std::collections::HashSet::new();
    for e in input
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Container && e.parent_id.as_deref() == Some(parent))
    {
        ids.insert(e.id.as_str());
        out.push_str(&format!(
            "    Container({}, \"{}\", \"{}\", \"{}\")\n",
            sanitize_alias(&e.id),
            c4_label(&e.name),
            c4_field(e.technology.as_deref()),
            c4_field(e.description.as_deref())
        ));
    }
    if ids.is_empty() {
        out.push_str(
            "    Container(empty, \"No containers yet\", \"n/a\", \"upsert_element kind=container\")\n",
        );
        out.push_str("  }\n");
        return out;
    }
    out.push_str("  }\n");

    // Also show people / external systems that relate to these containers
    for e in input.elements {
        if ids.contains(e.id.as_str()) {
            continue;
        }
        let touches = input.relationships.iter().any(|r| {
            (ids.contains(r.from_id.as_str()) && r.to_id == e.id)
                || (ids.contains(r.to_id.as_str()) && r.from_id == e.id)
        });
        if !touches {
            continue;
        }
        match e.kind {
            ElementKind::Person => {
                ids.insert(e.id.as_str());
                out.push_str(&format!(
                    "  Person({}, \"{}\", \"{}\")\n",
                    sanitize_alias(&e.id),
                    c4_label(&e.name),
                    c4_field(e.description.as_deref())
                ));
            }
            ElementKind::SoftwareSystem if e.id != parent => {
                ids.insert(e.id.as_str());
                let (_, desc) = external_desc(e.description.as_deref());
                out.push_str(&format!(
                    "  System_Ext({}, \"{}\", \"{}\")\n",
                    sanitize_alias(&e.id),
                    c4_label(&e.name),
                    c4_field(Some(&desc))
                ));
            }
            _ => {}
        }
    }

    emit_rels(&mut out, input, &ids);
    out.push_str("  UpdateLayoutConfig($c4ShapeInRow=\"3\", $c4BoundaryInRow=\"1\")\n");
    out
}

fn c4_component(input: &DiagramInput<'_>, parent_id: Option<&str>) -> String {
    let Some(parent) = parent_id else {
        return String::from(
            "C4Component\n  title Components\n  Container(empty, \"Pick a container parent\", \"n/a\", \"n/a\")\n",
        );
    };
    let parent_el = input.elements.iter().find(|e| e.id == parent);
    let title = parent_el
        .map(|e| format!("Components - {}", e.name))
        .unwrap_or_else(|| "Components".into());

    let mut out = String::from("C4Component\n");
    out.push_str(&format!("  title {}\n", c4_label(&title)));
    out.push_str(&format!(
        "  Container_Boundary({}, \"{}\") {{\n",
        sanitize_alias(&format!("{parent}_b")),
        c4_label(parent_el.map(|e| e.name.as_str()).unwrap_or(parent))
    ));

    let mut ids = std::collections::HashSet::new();
    for e in input
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Component && e.parent_id.as_deref() == Some(parent))
    {
        ids.insert(e.id.as_str());
        out.push_str(&format!(
            "    Component({}, \"{}\", \"{}\", \"{}\")\n",
            sanitize_alias(&e.id),
            c4_label(&e.name),
            c4_field(e.technology.as_deref()),
            c4_field(e.description.as_deref())
        ));
    }
    if ids.is_empty() {
        // Placeholder MUST stay inside the boundary; empty "" args break Mermaid 11.
        out.push_str(
            "    Component(empty, \"No components yet\", \"n/a\", \"upsert_element kind=component\")\n",
        );
        out.push_str("  }\n");
        return out;
    }
    out.push_str("  }\n");
    emit_rels(&mut out, input, &ids);
    out.push_str("  UpdateLayoutConfig($c4ShapeInRow=\"3\", $c4BoundaryInRow=\"1\")\n");
    out
}

/// C4 Level 4: Mermaid has no C4Code — use UML `classDiagram` (ADR 0004).
fn code_class_diagram(input: &DiagramInput<'_>, parent_id: Option<&str>) -> String {
    let Some(parent) = parent_id else {
        return String::from(
            "classDiagram\n  direction TB\n  class Empty {\n    pick a component parent\n  }\n",
        );
    };
    let els: Vec<&Element> = input
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Code && e.parent_id.as_deref() == Some(parent))
        .collect();
    if els.is_empty() {
        return String::from(
            "classDiagram\n  direction TB\n  class Empty {\n    no code elements yet\n  }\n",
        );
    }
    let ids: std::collections::HashSet<&str> = els.iter().map(|e| e.id.as_str()).collect();
    let has_inherit = input.relationships.iter().any(|r| {
        ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str()) && {
            let d = r.description.as_deref().unwrap_or("").to_ascii_lowercase();
            d.contains("extends") || d.contains("inherit") || d.contains("implements")
        }
    });
    // Inheritance reads better top→bottom; flat deps left→right.
    let mut out = format!(
        "classDiagram\n  direction {}\n",
        if has_inherit { "TB" } else { "LR" }
    );

    // Who is implemented/extended → stereotype Interface / Abstract
    let mut interfaces: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut bases: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for r in input.relationships {
        if !(ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str())) {
            continue;
        }
        let d = r.description.as_deref().unwrap_or("").to_ascii_lowercase();
        if d.contains("implements") {
            interfaces.insert(r.to_id.as_str());
        } else if d.contains("extends") || d.contains("inherit") {
            bases.insert(r.to_id.as_str());
        }
    }

    let ns = sanitize_alias(parent);
    let parent_name = input
        .elements
        .iter()
        .find(|e| e.id == parent)
        .map(|e| e.name.as_str())
        .unwrap_or(parent);
    out.push_str(&format!(
        "  namespace {}[\"{}\"] {{\n",
        ns,
        c4_label(parent_name)
    ));

    let mut alias_of: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    for e in &els {
        let alias = sanitize_alias(&e.id);
        alias_of.insert(e.id.as_str(), alias.clone());
        let label = c4_label(&e.name);
        if sanitize_alias(&e.name) != alias {
            out.push_str(&format!("    class {alias}[\"{label}\"] {{\n"));
        } else {
            out.push_str(&format!("    class {alias} {{\n"));
        }
        let members = class_members_from_element(e);
        if members.is_empty() {
            out.push_str("      +…()\n");
        } else {
            for member in &members {
                out.push_str(&format!("      {member}\n"));
            }
        }
        out.push_str("    }\n");
        // Meaningful stereotypes only — never emit empty <<Cls>> clutter.
        if interfaces.contains(e.id.as_str()) {
            out.push_str(&format!("    class {alias} <<Interface>>\n"));
        } else if bases.contains(e.id.as_str()) {
            out.push_str(&format!("    class {alias} <<Base>>\n"));
        }
    }
    out.push_str("  }\n");

    for r in input.relationships {
        if !(ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str())) {
            continue;
        }
        let from = alias_of
            .get(r.from_id.as_str())
            .map(String::as_str)
            .unwrap_or("x");
        let to = alias_of
            .get(r.to_id.as_str())
            .map(String::as_str)
            .unwrap_or("y");
        let desc = r.description.as_deref().unwrap_or("").to_ascii_lowercase();
        if desc.contains("extends") || desc.contains("inherit") {
            out.push_str(&format!("  {to} <|-- {from}\n"));
        } else if desc.contains("implements") {
            out.push_str(&format!("  {to} <|.. {from} : implements\n"));
        } else {
            let label = c4_label(r.description.as_deref().unwrap_or("uses"));
            out.push_str(&format!("  {from} --> {to} : {label}\n"));
        }
    }
    // NOTE: do NOT emit Mermaid `click` here — C4Context/Container and classDiagram
    // reject `click … _blank` (breaks whole diagram). Sources sidebar + WASM handle urls.
    out
}

#[allow(dead_code)] // exercised by unit tests; stereotypes also set via Rel keywords
fn class_stereo(s: &str) -> String {
    let t = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    match t.to_ascii_lowercase().as_str() {
        "" | "class" | "cls" => "Class".into(),
        "interface" => "Interface".into(),
        "enum" => "Enum".into(),
        "base" => "Base".into(),
        _ => t,
    }
}

fn class_members_from_element(e: &Element) -> Vec<String> {
    element_uml_members(e)
        .into_iter()
        .map(|s| sanitize_class_member(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

#[allow(dead_code)]
fn class_members(description: Option<&str>) -> Vec<String> {
    let el = Element {
        id: "_".into(),
        workspace_id: "_".into(),
        kind: ElementKind::Code,
        parent_id: None,
        name: "_".into(),
        description: description.map(str::to_string),
        technology: None,
        url: None,
        members: vec![],
    };
    class_members_from_element(&el)
}

/// Structured legend card (C4 / classDiagram) — Structurizr + mermaid-studio contrast rules.
pub fn legend_block(layer: C4Layer) -> String {
    let (title, items): (&str, &[(&str, &str, &str)]) = match layer {
        C4Layer::Context | C4Layer::Landscape => (
            "System context",
            &[
                ("person", "Person", "Human actor"),
                ("system", "Software system", "System of interest"),
                ("system-ext", "External system", "Outside the boundary"),
            ],
        ),
        C4Layer::Container => (
            "Containers",
            &[
                ("container", "Container", "App / DB / queue"),
                ("person", "Person", "Human actor"),
                ("system-ext", "External system", "Outside the boundary"),
            ],
        ),
        C4Layer::Component => (
            "Components",
            &[("component", "Component", "Logical building block")],
        ),
        C4Layer::Code => (
            "Code (UML classDiagram / WASM)",
            &[
                ("code-cls", "Class", "Name + members compartments"),
                ("code-iface", "Interface", "«Interface» stereotype"),
                (
                    "code-rel",
                    "extends / implements",
                    "Hollow △; implements dashed",
                ),
            ],
        ),
        C4Layer::Adr => return String::new(),
    };
    let mut rows = String::new();
    for (swatch, name, desc) in items {
        rows.push_str(&format!(
            r#"<li class="legend-item"><span class="swatch {swatch}" aria-hidden="true"></span><div><strong>{name}</strong><span class="desc">{desc}</span></div></li>"#
        ));
    }
    format!(
        r#"<details class="legend-panel" aria-label="Diagram legend" id="c4-legend">
  <summary class="legend-head"><h2>Legend</h2><span class="legend-sub">{title}</span></summary>
  <ul class="legend-list">{rows}</ul>
</details>"#
    )
}

/// Mermaid `base` themeVariables per layer (pastel fills + dark text).
pub fn mermaid_theme_vars(layer: C4Layer) -> &'static str {
    match layer {
        C4Layer::Code => {
            // Softer UML cards: lavender fill, indigo border, slate text, thicker edges.
            r#"{
        darkMode: false,
        background: '#f8fafc',
        primaryColor: '#eef2ff',
        primaryTextColor: '#1e1b4b',
        primaryBorderColor: '#4f46e5',
        secondaryColor: '#ecfdf5',
        secondaryTextColor: '#065f46',
        tertiaryColor: '#f8fafc',
        tertiaryTextColor: '#334155',
        lineColor: '#64748b',
        textColor: '#0f172a',
        mainBkg: '#eef2ff',
        classText: '#1e1b4b',
        noteBkgColor: '#fffbeb',
        noteTextColor: '#78350f',
        noteBorderColor: '#d97706',
        fontSize: '16px',
        fontFamily: 'ui-sans-serif, system-ui, sans-serif'
      }"#
        }
        _ => {
            r#"{
        darkMode: false,
        background: '#f8fafc',
        primaryColor: '#1168BD',
        primaryTextColor: '#ffffff',
        primaryBorderColor: '#0B4884',
        secondaryColor: '#23A2D9',
        secondaryTextColor: '#ffffff',
        tertiaryColor: '#e2e8f0',
        tertiaryTextColor: '#334155',
        lineColor: '#94a3b8',
        textColor: '#334155',
        fontSize: '15px'
      }"#
        }
    }
}

/// Mermaid 11 classDiagram: drop `*` / dunders; snake_case → camelCase (keeps readable names).
fn sanitize_class_member(s: &str) -> String {
    let mut raw = String::with_capacity(s.len());
    for c in s.chars() {
        let ok = matches!(
            c,
            'A'..='Z'
                | 'a'..='z'
                | '0'..='9'
                | '+'
                | '-'
                | '#'
                | '~'
                | '('
                | ')'
                | '['
                | ']'
                | ','
                | '.'
                | '_'
                | ' '
                | ':'
                | '?'
        );
        if ok {
            raw.push(c);
        }
        // drop *; keep ':' for typed UML params.
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let out = snake_member_to_camel(&collapsed);
    // Empty stubs like `+()` (from `+…()` after stripping) break readability — drop them.
    let core = out
        .trim_start_matches(['+', '-', '#', '~'])
        .trim_matches(['(', ')', '.', ' ']);
    if core.is_empty() {
        String::new()
    } else {
        out
    }
}

/// `+create_logger()` → `+createLogger()`; `+_dispatch()` → `+dispatch()`.
fn snake_member_to_camel(s: &str) -> String {
    let (vis, rest) = if let Some(r) = s.strip_prefix('+') {
        ("+", r)
    } else if let Some(r) = s.strip_prefix('-') {
        ("-", r)
    } else if let Some(r) = s.strip_prefix('#') {
        ("#", r)
    } else if let Some(r) = s.strip_prefix('~') {
        ("~", r)
    } else {
        ("", s)
    };
    let (ident, args) = match rest.find('(') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let ident = ident.trim_matches('_');
    let mut parts = ident.split('_').filter(|p| !p.is_empty());
    let mut name = parts.next().unwrap_or("").to_string();
    for p in parts {
        let mut chs = p.chars();
        if let Some(f) = chs.next() {
            name.extend(f.to_uppercase());
            name.push_str(chs.as_str());
        }
    }
    format!("{vis}{name}{args}")
}

fn emit_rels(out: &mut String, input: &DiagramInput<'_>, ids: &std::collections::HashSet<&str>) {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for r in input.relationships {
        if !(ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str())) {
            continue;
        }
        let from = sanitize_alias(&r.from_id);
        let to = sanitize_alias(&r.to_id);
        let label = c4_rel_label(r.description.as_deref());
        match r
            .technology
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => out.push_str(&format!("  Rel({from}, {to}, \"{label}\")\n")),
            Some(techn) => out.push_str(&format!(
                "  Rel({from}, {to}, \"{label}\", \"{}\")\n",
                c4_label(techn)
            )),
        }
        pairs.push((r.from_id.as_str(), r.to_id.as_str()));
    }
    // Soft lines + staggered label offsets so Mermaid C4 doesn't stack Rel text on top of itself.
    for (i, (from_id, to_id)) in pairs.iter().enumerate() {
        let from = sanitize_alias(from_id);
        let to = sanitize_alias(to_id);
        // Larger stagger — Mermaid C4 stacks Rel labels in the same corridor otherwise.
        let oy = -40 + (i as i32) * 22;
        let ox = -30 + (i as i32 % 4) * 22;
        out.push_str(&format!(
            "  UpdateRelStyle({from}, {to}, $textColor=\"#475569\", $lineColor=\"#94a3b8\", $offsetX=\"{ox}\", $offsetY=\"{oy}\")\n"
        ));
    }
}

/// Detect external systems; strip the marker so it never appears in the Mermaid description.
fn external_desc(description: Option<&str>) -> (bool, String) {
    let raw = description.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return (false, String::new());
    }
    let lower = raw.to_ascii_lowercase();
    let external = lower.starts_with("external ")
        || lower.starts_with("external:")
        || lower.starts_with("[external]")
        || lower == "external";
    let cleaned = if lower.starts_with("external ") {
        raw["external ".len()..].trim().to_string()
    } else if lower.starts_with("external:") {
        raw["external:".len()..].trim().to_string()
    } else if lower.starts_with("[external]") {
        raw["[external]".len()..].trim().to_string()
    } else if lower == "external" {
        String::new()
    } else if lower.contains("external") {
        // legacy heuristic (contains word) — still mark external, strip the word token
        raw.split_whitespace()
            .filter(|w| !w.eq_ignore_ascii_case("external"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        raw.to_string()
    };
    let external = external || lower.split_whitespace().any(|w| w == "external");
    (external, cleaned)
}

/// Short Rel labels — long sentences collide in Mermaid C4 layout.
fn c4_rel_label(description: Option<&str>) -> String {
    let raw = description
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("uses");
    let s = mermaid_edge_label(raw);
    const MAX: usize = 36;
    if s.chars().count() <= MAX {
        return s;
    }
    let mut out: String = s.chars().take(MAX.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

/// Mermaid flowchart edge text: `*` toggles emphasis, `|` ends the label,
/// `→`/brackets confuse the tokenizer. Keep ASCII-safe short captions.
fn mermaid_edge_label(raw: &str) -> String {
    c4_label(raw)
        .replace('*', "x")
        .replace('→', "->")
        .replace('←', "<-")
        .replace('|', "/")
        .replace('#', "")
        .replace('<', "(")
        .replace('>', ")")
}

/// Drill targets for the HTML sidebar (replaces unreadable Mermaid tooltips).
pub fn drill_targets<'a>(
    elements: &'a [Element],
    layer: C4Layer,
    parent_id: Option<&str>,
) -> Vec<&'a Element> {
    match layer {
        C4Layer::Context | C4Layer::Landscape => elements
            .iter()
            .filter(|e| e.kind == ElementKind::SoftwareSystem)
            .collect(),
        C4Layer::Container => elements
            .iter()
            .filter(|e| e.kind == ElementKind::Container && e.parent_id.as_deref() == parent_id)
            .collect(),
        C4Layer::Component => elements
            .iter()
            .filter(|e| e.kind == ElementKind::Component && e.parent_id.as_deref() == parent_id)
            .collect(),
        C4Layer::Code | C4Layer::Adr => Vec::new(),
    }
}

/// One step up the C4 hierarchy (code→component→container→context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillUp {
    pub layer: C4Layer,
    pub parent_id: Option<String>,
    /// Short label for the destination (usually parent element name).
    pub label: String,
    pub kind_label: &'static str,
}

pub fn drill_up_target(
    elements: &[Element],
    layer: C4Layer,
    parent_id: Option<&str>,
    mode_all: bool,
) -> Option<DrillUp> {
    if mode_all {
        // All+focus → full All (drop focus). Bare All has nowhere further up.
        return parent_id.map(|_| DrillUp {
            layer: C4Layer::Context,
            parent_id: None,
            label: "All layers".into(),
            kind_label: "all",
        });
    }
    let pid = parent_id?;
    let parent = elements.iter().find(|e| e.id == pid)?;
    match layer {
        C4Layer::Code => {
            // parent is component → component diagram of its container
            let container_id = parent.parent_id.clone()?;
            let container_name = elements
                .iter()
                .find(|e| e.id == container_id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| container_id.clone());
            Some(DrillUp {
                layer: C4Layer::Component,
                parent_id: Some(container_id),
                label: container_name,
                kind_label: "component",
            })
        }
        C4Layer::Component => {
            let system_id = parent.parent_id.clone()?;
            let system_name = elements
                .iter()
                .find(|e| e.id == system_id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| system_id.clone());
            Some(DrillUp {
                layer: C4Layer::Container,
                parent_id: Some(system_id),
                label: system_name,
                kind_label: "container",
            })
        }
        C4Layer::Container => Some(DrillUp {
            layer: C4Layer::Context,
            parent_id: None,
            label: "Context".into(),
            kind_label: "context",
        }),
        C4Layer::Context | C4Layer::Landscape | C4Layer::Adr => None,
    }
}

fn drill_up_href(base: &str, up: &DrillUp, mode_all_focus: bool, use_wasm: bool) -> String {
    let renderer = if use_wasm { "wasm" } else { "mermaid" };
    if mode_all_focus && up.kind_label == "all" {
        return format!("{base}/?mode=all&renderer={renderer}");
    }
    match &up.parent_id {
        Some(p) => format!(
            "{base}/?layer={}&parent={}&renderer={renderer}",
            up.layer.as_str(),
            urlencoding(p)
        ),
        None => format!(
            "{base}/?layer={}&renderer={renderer}",
            up.layer.as_str()
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn view_html(
    workspace_id: &str,
    layer: C4Layer,
    parent_id: Option<&str>,
    mermaid: &str,
    base_url: &str,
    up_parent_id: Option<&str>,
    elements: &[Element],
    adr_count: usize,
    flow_count: usize,
    mode: &str,
    renderer: &str,
    scene_json: &str,
) -> String {
    let base = base_url.trim_end_matches('/');
    let mode_all = ViewMode::parse(mode) == ViewMode::All;
    let use_wasm = matches!(renderer, "wasm" | "auto" | "webgpu" | "canvas2d");

    let crumbs = if mode_all {
        let renderer = if use_wasm { "wasm" } else { "mermaid" };
        if let Some(focus) = parent_id {
            format!(
                r#"<nav class="crumbs"><a href="{base}/?mode=all&amp;renderer={renderer}">All layers</a><span class="sep">/</span> focus <code>{focus}</code></nav>"#,
                base = base,
                renderer = renderer,
                focus = html_escape(focus),
            )
        } else {
            r#"<nav class="crumbs"><span class="here">All layers</span></nav>"#.into()
        }
    } else {
        breadcrumb_html(base, workspace_id, layer, parent_id, up_parent_id, elements)
    };
    let diagrams_active = if mode_all { "" } else { " active" };
    let all_active = if mode_all { " active" } else { "" };
    let focus_q = parent_id
        .map(|p| format!("&focus={}", urlencoding(p)))
        .unwrap_or_default();
    // Preserve current view when switching renderer (All vs layer drill).
    let view_q = if mode_all {
        format!("mode=all{focus_q}")
    } else {
        let mut q = format!("layer={}", layer.as_str());
        if let Some(p) = parent_id {
            q.push_str(&format!("&parent={}", urlencoding(p)));
        }
        q
    };
    let mermaid_active = if use_wasm { "" } else { " active" };
    let wasm_active = if use_wasm { " active" } else { "" };
    let renderer_label = if use_wasm { "WASM" } else { "Mermaid" };
    let top_nav = format!(
        r#"<nav class="top-tabs" aria-label="Primary">
      <a class="tab{diagrams_active}" href="{base}/?layer=context">Diagrams</a>
      <a class="tab{all_active}" href="{base}/?mode=all&amp;renderer=wasm{focus_q}">All</a>
      <a class="tab" href="{base}/flows">Flows ({nf})</a>
      <a class="tab" href="{base}/adrs">ADRs ({n})</a>
    </nav>
    <div class="renderer-switch" role="group" aria-label="Renderer">
      <span class="renderer-label">Render <strong>{renderer_label}</strong></span>
      <nav class="top-tabs renderer-tabs" aria-label="Choose renderer">
        <a class="tab{mermaid_active}" href="{base}/?{view_q}&amp;renderer=mermaid" title="Mermaid SVG">Mermaid</a>
        <a class="tab{wasm_active}" href="{base}/?{view_q}&amp;renderer=wasm" title="WASM Canvas2D">WASM</a>
      </nav>
    </div>"#,
        base = base,
        n = adr_count,
        nf = flow_count,
        diagrams_active = diagrams_active,
        all_active = all_active,
        focus_q = focus_q,
        view_q = view_q,
        mermaid_active = mermaid_active,
        wasm_active = wasm_active,
        renderer_label = renderer_label,
    );
    let drills = drill_targets(elements, layer, parent_id);
    let drill_up = drill_up_target(elements, layer, parent_id, mode_all);
    let mut drill_html = String::new();
    let mut toolbar_up = String::new();
    if let Some(ref up) = drill_up {
        let href = drill_up_href(base, up, mode_all && parent_id.is_some(), use_wasm);
        // Desktop panel (not open by default — JS opens on desktop only).
        drill_html.push_str(&format!(
            r#"<details class="drills drill-up" id="c4-drill-up"><summary><h2>Drill up</h2></summary><ul><li><a class="drill up" href="{href}"><span class="kind">{kind}</span><span class="name">{name}</span><span class="chev">↑</span></a></li></ul></details>"#,
            href = href,
            kind = html_escape(up.kind_label),
            name = html_escape(&up.label),
        ));
        // Mobile: big ↑ in the zoom toolbar (no floating overlay).
        toolbar_up = format!(
            r#"<a class="toolbar-up" href="{href}" title="Drill up to {name}" aria-label="Drill up">{chev} {name}</a>"#,
            href = href,
            name = html_escape(&up.label),
            chev = "↑",
        );
    }
    if !drills.is_empty() {
        drill_html.push_str(
            "<details class=\"drills\" id=\"c4-drills\"><summary><h2>Drill down</h2></summary><ul>",
        );
        for e in drills {
            if let Some(next) = e.kind.drill_layer() {
                drill_html.push_str(&format!(
                    r#"<li><a class="drill" href="{base}/?layer={layer}&parent={pid}"><span class="kind">{kind}</span><span class="name">{name}</span><span class="chev">→</span></a></li>"#,
                    base = base,
                    layer = next.as_str(),
                    pid = urlencoding(&e.id),
                    kind = html_escape(e.kind.as_str()),
                    name = html_escape(&e.name),
                ));
            }
        }
        drill_html.push_str("</ul></details>");
    }
    // External source links for elements visible in this layer (Ceph → GitHub, etc.).
    let mut source_html = String::new();
    let sources: Vec<&Element> = match layer {
        C4Layer::Code => elements
            .iter()
            .filter(|e| {
                e.kind == ElementKind::Code
                    && e.parent_id.as_deref() == parent_id
                    && e.url.as_deref().is_some_and(|u| u.starts_with("https://"))
            })
            .collect(),
        C4Layer::Component => elements
            .iter()
            .filter(|e| {
                e.kind == ElementKind::Component
                    && e.parent_id.as_deref() == parent_id
                    && e.url.as_deref().is_some_and(|u| u.starts_with("https://"))
            })
            .collect(),
        C4Layer::Container => elements
            .iter()
            .filter(|e| {
                e.kind == ElementKind::Container
                    && e.parent_id.as_deref() == parent_id
                    && e.url.as_deref().is_some_and(|u| u.starts_with("https://"))
            })
            .collect(),
        C4Layer::Context | C4Layer::Landscape => elements
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ElementKind::Person | ElementKind::SoftwareSystem | ElementKind::External
                ) && e.url.as_deref().is_some_and(|u| u.starts_with("https://"))
            })
            .collect(),
        _ => Vec::new(),
    };
    if !sources.is_empty() {
        source_html.push_str(
            "<details class=\"drills sources\" id=\"c4-sources\"><summary><h2>Sources</h2></summary><ul>",
        );
        for e in sources.into_iter().take(40) {
            let url = e.url.as_deref().unwrap_or("#");
            source_html.push_str(&format!(
                r#"<li><a class="drill" href="{url}" target="_blank" rel="noopener noreferrer"><span class="kind">link</span><span class="name">{name}</span><span class="chev">↗</span></a></li>"#,
                url = html_escape(url),
                name = html_escape(&e.name),
            ));
        }
        source_html.push_str("</ul></details>");
    }
    drill_html.push_str(&source_html);

    let legend = if mode_all {
        r#"<details class="legend-panel" aria-label="Diagram legend" id="c4-legend">
  <summary class="legend-head"><h2>Legend</h2><span class="legend-sub">All C4 layers</span></summary>
  <ul class="legend-list">
    <li class="legend-item"><span class="swatch system" aria-hidden="true"></span><div><strong>Context</strong><span class="desc">Person / system</span></div></li>
    <li class="legend-item"><span class="swatch container" aria-hidden="true"></span><div><strong>Container</strong><span class="desc">App / DB</span></div></li>
    <li class="legend-item"><span class="swatch component" aria-hidden="true"></span><div><strong>Component</strong><span class="desc">Logical block</span></div></li>
    <li class="legend-item"><span class="swatch code-cls" aria-hidden="true"></span><div><strong>Code</strong><span class="desc">Class / type</span></div></li>
    <li class="legend-item"><span class="swatch" style="background:#fff;border:2px solid #312e81;transform:rotate(45deg);width:10px;height:10px;border-radius:1px" aria-hidden="true"></span><div><strong>Viewpoint ◇</strong><span class="desc">Border port — arrows dock here, not center (WASM)</span></div></li>
    <li class="legend-item"><span class="swatch" style="background:#4338ca;border-radius:2px" aria-hidden="true"></span><div><strong>Edge</strong><span class="desc">Caption <code>From → To · uses</code> sits <strong>above</strong> the edge (Structurizr/ELK-style)</span></div></li>
  </ul>
</details>"#
        .to_string()
    } else {
        legend_block(layer)
    };
    // WASM: redraw on zoom (setTransform). Mermaid SVG: CSS transform stays sharp (vectors).
    let stage_inner = if use_wasm {
        let mermaid_fallback = format!(
            "{base}/?mode=all&renderer=mermaid{focus}",
            base = base,
            focus = parent_id
                .map(|p| format!("&focus={}", urlencoding(p)))
                .unwrap_or_default(),
        );
        format!(
            r#"<canvas id="c4-canvas" aria-label="C4 WASM canvas"></canvas>
    <script type="application/json" id="c4-scene">{scene}</script>
    <div id="c4-boot-status" class="boot-status" role="status">Loading WASM viewer…</div>
    <div class="board-toolbar" aria-label="Board zoom">
      {toolbar_up}
      <button type="button" data-zoom="-" aria-label="Zoom out">−</button>
      <button type="button" data-zoom="reset" aria-label="Fit diagram">Fit</button>
      <button type="button" data-zoom="+" aria-label="Zoom in">+</button>
      <a class="fallback-link" href="{fallback}">Mermaid</a>
    </div>
    <script type="module">
      const status = document.getElementById('c4-boot-status');
      const setStatus = (msg, isErr) => {{
        if (!status) return;
        status.textContent = msg;
        status.classList.toggle('error', !!isErr);
        status.hidden = !msg;
      }};
      try {{
        // Cache-bust: browsers may keep a broken older wasm/js for up to max-age.
        const bust = 'v=20260718hover3';
        const mod = await import('/wasm/architect_c4_wasm.js?' + bust);
        const init = mod.default;
        const {{ render_scene, preferred_backend, hit_test_edge, hit_test_node }} = mod;
        const sceneEl = document.getElementById('c4-scene');
        const scene = sceneEl ? sceneEl.textContent : '';
        if (!scene || scene.trim()[0] !== '{{') {{
          throw new Error('scene JSON missing or still HTML-escaped');
        }}
        JSON.parse(scene); // fail fast
        const want = {want};
        const stage = document.querySelector('.stage');
        const canvas = document.getElementById('c4-canvas');
        let scale = 1, tx = 40, ty = 40, dragging = false, lx = 0, ly = 0, raf = 0;
        let fitScale = 1, userZoomed = false, sceneW = 1, sceneH = 1;
        let sceneGraph = null, ptrDown = null;
        const pointers = new Map();
        let pinch = null; // {{ dist0, scale0 }}
        let didPinch = false;
        let hoverEdgeId = '';
        let hoverNodeId = '';
        try {{
          sceneGraph = JSON.parse(scene);
          sceneW = Math.max(sceneGraph.width, 1);
          sceneH = Math.max(sceneGraph.height, 1);
        }} catch (_) {{}}
        const hitNode = (cssX, cssY) => {{
          if (!sceneGraph || !sceneGraph.nodes) return null;
          const sx = (cssX - tx) / scale;
          const sy = (cssY - ty) / scale;
          let hit = null;
          for (const n of sceneGraph.nodes) {{
            if (n.group) continue;
            if (sx >= n.x && sx <= n.x + n.w && sy >= n.y && sy <= n.y + n.h) hit = n;
          }}
          return hit;
        }};
        const worldFromCss = (cssX, cssY) => ({{
          x: (cssX - tx) / scale,
          y: (cssY - ty) / scale,
        }});
        setStatus('Initializing WASM…');
        await init({{ module_or_path: '/wasm/architect_c4_wasm_bg.wasm?' + bust }});
        const pref = preferred_backend();
        const backend = want === 'auto' ? (pref === 'webgpu' ? 'webgpu' : 'canvas2d') : want;
        const label = () => {{
          const b = document.querySelector('[data-zoom="reset"]');
          if (!b) return;
          if (!userZoomed || Math.abs(scale - fitScale) < 0.01) b.textContent = 'Fit';
          else b.textContent = Math.round((scale / fitScale) * 100) + '%';
        }};
        const redraw = () => {{
          raf = 0;
          try {{
            render_scene('c4-canvas', scene, backend, scale, tx, ty, hoverEdgeId || '', hoverNodeId || '');
            setStatus('');
          }} catch (err) {{
            console.error('architect-c4-wasm render_scene failed', err);
            setStatus('WASM render failed: ' + err, true);
          }}
          label();
        }};
        const schedule = () => {{ if (!raf) raf = requestAnimationFrame(() => {{ redraw(); }}); }};
        const updateHover = (cssX, cssY) => {{
          const w = worldFromCss(cssX, cssY);
          // Prefer edge/note; else class/node highlight (connected edges light up separately).
          let eid = (typeof hit_test_edge === 'function')
            ? (hit_test_edge(scene, w.x, w.y, 12 / scale) || '') : '';
          let nid = '';
          if (!eid && typeof hit_test_node === 'function') {{
            nid = hit_test_node(scene, w.x, w.y) || '';
          }} else if (!eid) {{
            const n = hitNode(cssX, cssY);
            nid = n ? n.id : '';
          }}
          if (eid !== hoverEdgeId || nid !== hoverNodeId) {{
            hoverEdgeId = eid;
            hoverNodeId = nid;
            stage.style.cursor = (eid || nid) ? 'pointer' : '';
            schedule();
          }}
        }};
        const fit = () => {{
          const sw = Math.max(stage.clientWidth || window.innerWidth, 280);
          const sh = Math.max(stage.clientHeight || (window.innerHeight - 56), 200);
          const padX = 16, padY = 16;
          const mobile = window.matchMedia('(max-width: 720px)').matches;
          const sx = (sw - padX * 2) / sceneW;
          const sy = (sh - padY * 2) / sceneH;
          // Mobile: contain (whole diagram visible). Desktop: width-first fill.
          fitScale = Math.max(0.05, mobile ? Math.min(sx, sy) : sx);
          scale = fitScale;
          tx = (sw - sceneW * scale) / 2;
          const drawnH = sceneH * scale;
          ty = drawnH <= sh ? (sh - drawnH) / 2 : padY;
          userZoomed = false;
          redraw();
        }};
        const zoomAt = (cssX, cssY, next) => {{
          next = Math.min(fitScale * 8, Math.max(fitScale * 0.15, next));
          const k = next / scale;
          tx = cssX - (cssX - tx) * k;
          ty = cssY - (cssY - ty) * k;
          scale = next;
          userZoomed = true;
          schedule();
          label();
        }};
        const pinchDist = () => {{
          const pts = [...pointers.values()];
          if (pts.length < 2) return 0;
          return Math.hypot(pts[0].x - pts[1].x, pts[0].y - pts[1].y);
        }};
        const pinchMid = () => {{
          const pts = [...pointers.values()];
          const rect = stage.getBoundingClientRect();
          return {{
            x: (pts[0].x + pts[1].x) / 2 - rect.left,
            y: (pts[0].y + pts[1].y) / 2 - rect.top,
          }};
        }};
        stage.addEventListener('wheel', (e) => {{
          e.preventDefault();
          const rect = stage.getBoundingClientRect();
          zoomAt(e.clientX - rect.left, e.clientY - rect.top, scale * (e.deltaY > 0 ? 0.9 : 1.1));
        }}, {{ passive: false }});
        stage.addEventListener('pointerdown', (e) => {{
          if (e.pointerType === 'mouse' && e.button !== 0) return;
          pointers.set(e.pointerId, {{ x: e.clientX, y: e.clientY }});
          try {{ stage.setPointerCapture(e.pointerId); }} catch (_) {{}}
          if (pointers.size >= 2) {{
            dragging = false;
            ptrDown = null;
            didPinch = true;
            const d0 = pinchDist();
            if (d0 > 8) pinch = {{ dist0: d0, scale0: scale }};
            stage.classList.remove('grabbing');
            return;
          }}
          didPinch = false;
          dragging = true; lx = e.clientX; ly = e.clientY;
          const rect = stage.getBoundingClientRect();
          ptrDown = {{ x: e.clientX, y: e.clientY, cssX: e.clientX - rect.left, cssY: e.clientY - rect.top, moved: 0 }};
          stage.classList.add('grabbing');
        }});
        stage.addEventListener('pointermove', (e) => {{
          const rect = stage.getBoundingClientRect();
          const cssX = e.clientX - rect.left;
          const cssY = e.clientY - rect.top;
          if (!pointers.has(e.pointerId)) {{
            // Hover-only (mouse not captured): highlight edges under cursor.
            if (e.pointerType === 'mouse' && !dragging && pointers.size === 0) {{
              updateHover(cssX, cssY);
            }}
            return;
          }}
          pointers.set(e.pointerId, {{ x: e.clientX, y: e.clientY }});
          if (pointers.size >= 2 && pinch) {{
            didPinch = true;
            const d = pinchDist();
            if (d > 8 && pinch.dist0 > 8) {{
              const mid = pinchMid();
              zoomAt(mid.x, mid.y, pinch.scale0 * (d / pinch.dist0));
            }}
            return;
          }}
          if (!dragging) return;
          if (ptrDown) ptrDown.moved += Math.abs(e.clientX - lx) + Math.abs(e.clientY - ly);
          tx += e.clientX - lx; ty += e.clientY - ly;
          lx = e.clientX; ly = e.clientY;
          if (hoverEdgeId || hoverNodeId) {{
            hoverEdgeId = ''; hoverNodeId = ''; stage.style.cursor = '';
          }}
          schedule();
        }});
        stage.addEventListener('pointerleave', () => {{
          if ((hoverEdgeId || hoverNodeId) && pointers.size === 0) {{
            hoverEdgeId = ''; hoverNodeId = '';
            stage.style.cursor = '';
            schedule();
          }}
        }});
        const endPointer = (e) => {{
          pointers.delete(e.pointerId);
          if (pointers.size < 2) pinch = null;
          if (pointers.size === 0) {{
            if (ptrDown && ptrDown.moved < 6 && !didPinch) {{
              const n = hitNode(ptrDown.cssX, ptrDown.cssY);
              if (n && n.url && String(n.url).startsWith('https://')) {{
                window.open(n.url, '_blank', 'noopener,noreferrer');
              }}
            }}
            ptrDown = null;
            dragging = false;
            didPinch = false;
            stage.classList.remove('grabbing');
          }} else if (pointers.size === 1) {{
            const p = [...pointers.values()][0];
            dragging = true; lx = p.x; ly = p.y;
            ptrDown = null;
          }}
        }};
        stage.addEventListener('pointerup', endPointer);
        stage.addEventListener('pointercancel', endPointer);
        document.querySelectorAll('.board-toolbar [data-zoom]').forEach((btn) => {{
          btn.addEventListener('click', () => {{
            const z = btn.getAttribute('data-zoom');
            const rect = stage.getBoundingClientRect();
            const cx = rect.width / 2, cy = rect.height / 2;
            if (z === '+') zoomAt(cx, cy, scale * 1.2);
            else if (z === '-') zoomAt(cx, cy, scale / 1.2);
            else fit();
          }});
        }});
        // Continuous contain-fit on stage box changes (web.dev / WebGLFundamentals ResizeObserver pattern).
        const ro = new ResizeObserver(() => {{
          if (!userZoomed) fit();
          else schedule();
        }});
        ro.observe(stage);
        fit();
        console.info('architect-c4 renderer', {{ prefer: pref, backend, fit: 'ResizeObserver contain' }});
      }} catch (err) {{
        console.error('architect-c4-wasm boot failed', err);
        setStatus('WASM failed: ' + err + ' — open Mermaid fallback below.', true);
      }}
    </script>"#,
            scene = scene_json.replace("</", "<\\/"),
            want = serde_json::to_string(if renderer == "auto" {
                "auto"
            } else if renderer == "webgpu" {
                "webgpu"
            } else {
                "canvas2d"
            })
            .unwrap(),
            fallback = html_escape(&mermaid_fallback),
            toolbar_up = toolbar_up.as_str(),
        )
    } else {
        // SVG stays sharp under CSS transform (vectors). Still offer pan/zoom board UX.
        // Mermaid boot is inlined here so fit-to-view can set the same camera.
        let theme_vars = mermaid_theme_vars(if mode_all { C4Layer::Context } else { layer });
        format!(
            r#"<div class="board-world" id="board-world">
      <pre class="mermaid">
{mermaid}
      </pre>
    </div>
    <div class="board-toolbar" aria-label="Board zoom">
      {toolbar_up}
      <button type="button" data-zoom="-" aria-label="Zoom out">−</button>
      <button type="button" data-zoom="reset" aria-label="Fit diagram">Fit</button>
      <button type="button" data-zoom="+" aria-label="Zoom in">+</button>
    </div>
    <script type="module">
      import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
      mermaid.initialize({{
        startOnLoad: false,
        securityLevel: 'loose',
        theme: 'base',
        themeVariables: {theme_vars}
      }});
      await mermaid.run({{ querySelector: '.mermaid' }});
      const world = document.getElementById('board-world');
      const stage = document.querySelector('.stage');
      let scale = 1, tx = 0, ty = 0, dragging = false, lx = 0, ly = 0;
      let fitScale = 1, userZoomed = false, sceneW = 1, sceneH = 1;
      const pointers = new Map();
      let pinch = null, didPinch = false;
      const label = () => {{
        const b = document.querySelector('[data-zoom="reset"]');
        if (!b) return;
        if (!userZoomed || Math.abs(scale - fitScale) < 0.01) b.textContent = 'Fit';
        else b.textContent = Math.round((scale / fitScale) * 100) + '%';
      }};
      const apply = () => {{
        world.style.transform = `translate(${{tx}}px, ${{ty}}px) scale(${{scale}})`;
        label();
      }};
      const zoomAt = (cssX, cssY, next) => {{
        next = Math.min(fitScale * 8, Math.max(fitScale * 0.15, next));
        const k = next / scale;
        tx = cssX - (cssX - tx) * k;
        ty = cssY - (cssY - ty) * k;
        scale = next;
        userZoomed = true;
        apply();
      }};
      const pinchDist = () => {{
        const pts = [...pointers.values()];
        if (pts.length < 2) return 0;
        return Math.hypot(pts[0].x - pts[1].x, pts[0].y - pts[1].y);
      }};
      const pinchMid = () => {{
        const pts = [...pointers.values()];
        const rect = stage.getBoundingClientRect();
        return {{
          x: (pts[0].x + pts[1].x) / 2 - rect.left,
          y: (pts[0].y + pts[1].y) / 2 - rect.top,
        }};
      }};
      const normalizeSvg = (svg) => {{
        // Fix Mermaid width=100% so camera math uses real scene pixels.
        // Refs: CSS-Tricks How to Scale SVG; MDN preserveAspectRatio (meet ≈ contain).
        const bb = svg.getBBox();
        const w = Math.max(bb.width + 24, 1);
        const h = Math.max(bb.height + 24, 1);
        svg.setAttribute('viewBox', `${{bb.x - 12}} ${{bb.y - 12}} ${{w}} ${{h}}`);
        svg.removeAttribute('style');
        svg.setAttribute('width', String(w));
        svg.setAttribute('height', String(h));
        svg.style.cssText = `width:${{w}}px;height:${{h}}px;max-width:none;display:block`;
        sceneW = w; sceneH = h;
        return {{ w, h }};
      }};
      const fit = () => {{
        const svg = world.querySelector('svg');
        if (!svg) {{ apply(); return; }}
        // Measure at identity transform
        world.style.transform = 'none';
        normalizeSvg(svg);
        // Width-first fit: stretch to full stage width (user request).
        // Height may overflow → pan; ResizeObserver keeps width locked on window changes.
        const padX = 24, padY = 24;
        const sw = Math.max(stage.clientWidth - padX * 2, 200);
        const sh = Math.max(stage.clientHeight - padY * 2, 200);
        const mobile = window.matchMedia('(max-width: 720px)').matches;
        const sx = sw / Math.max(sceneW, 1);
        const sy = sh / Math.max(sceneH, 1);
        fitScale = Math.max(0.05, mobile ? Math.min(sx, sy) : sx);
        scale = fitScale;
        tx = (stage.clientWidth - sceneW * scale) / 2;
        const drawnH = sceneH * scale;
        ty = drawnH <= stage.clientHeight
          ? (stage.clientHeight - drawnH) / 2
          : padY;
        userZoomed = false;
        apply();
      }};
      stage.addEventListener('wheel', (e) => {{
        e.preventDefault();
        const rect = stage.getBoundingClientRect();
        zoomAt(e.clientX - rect.left, e.clientY - rect.top, scale * (e.deltaY > 0 ? 0.9 : 1.1));
      }}, {{ passive: false }});
      stage.addEventListener('pointerdown', (e) => {{
        if (e.pointerType === 'mouse' && e.button !== 0) return;
        pointers.set(e.pointerId, {{ x: e.clientX, y: e.clientY }});
        try {{ stage.setPointerCapture(e.pointerId); }} catch (_) {{}}
        if (pointers.size >= 2) {{
          dragging = false;
          didPinch = true;
          const d0 = pinchDist();
          if (d0 > 8) pinch = {{ dist0: d0, scale0: scale }};
          stage.classList.remove('grabbing');
          return;
        }}
        didPinch = false;
        dragging = true; lx = e.clientX; ly = e.clientY;
        stage.classList.add('grabbing');
      }});
      stage.addEventListener('pointermove', (e) => {{
        if (!pointers.has(e.pointerId)) return;
        pointers.set(e.pointerId, {{ x: e.clientX, y: e.clientY }});
        if (pointers.size >= 2 && pinch) {{
          didPinch = true;
          const d = pinchDist();
          if (d > 8 && pinch.dist0 > 8) {{
            const mid = pinchMid();
            zoomAt(mid.x, mid.y, pinch.scale0 * (d / pinch.dist0));
          }}
          return;
        }}
        if (!dragging) return;
        tx += e.clientX - lx; ty += e.clientY - ly;
        lx = e.clientX; ly = e.clientY; apply();
      }});
      const endPointer = (e) => {{
        pointers.delete(e.pointerId);
        if (pointers.size < 2) pinch = null;
        if (pointers.size === 0) {{
          dragging = false; didPinch = false;
          stage.classList.remove('grabbing');
        }} else if (pointers.size === 1) {{
          const p = [...pointers.values()][0];
          dragging = true; lx = p.x; ly = p.y;
        }}
      }};
      stage.addEventListener('pointerup', endPointer);
      stage.addEventListener('pointercancel', endPointer);
      document.querySelectorAll('.board-toolbar [data-zoom]').forEach((btn) => {{
        btn.addEventListener('click', () => {{
          const z = btn.getAttribute('data-zoom');
          const rect = stage.getBoundingClientRect();
          const cx = rect.width / 2, cy = rect.height / 2;
          if (z === '+') zoomAt(cx, cy, scale * 1.2);
          else if (z === '-') zoomAt(cx, cy, scale / 1.2);
          else fit();
        }});
      }});
      // Continuous scale vs window/stage size (ResizeObserver > window.resize).
      const ro = new ResizeObserver(() => {{ if (!userZoomed) fit(); }});
      ro.observe(stage);
      fit();
      console.info('architect-c4 mermaid fit', {{ mode: 'ResizeObserver contain' }});
    </script>"#,
            mermaid = html_escape(mermaid),
            theme_vars = theme_vars,
            toolbar_up = toolbar_up.as_str(),
        )
    };
    let mermaid_boot = String::new();

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>C4 {layer_title}</title>
  {mermaid_boot}
  <style>
    :root {{
      --bg: #f1f5f9;
      --panel: #ffffff;
      --ink: #0f172a;
      --muted: #64748b;
      --line: #e2e8f0;
      --accent: #4f46e5;
      --person: #08427B;
      --system: #1168BD;
      --system-ext: #94a3b8;
      --container: #23A2D9;
      --component: #a5b4fc;
      --code: #ddd6fe;
      --top: 56px;
    }}
    * {{ box-sizing: border-box; }}
    html, body {{ height: 100%; }}
    body.app-shell {{
      margin: 0;
      font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif;
      background: var(--bg);
      color: var(--ink);
      line-height: 1.45;
      overflow: hidden;
    }}
    .topbar {{
      position: fixed; inset: 0 0 auto 0; height: var(--top); z-index: 40;
      background: rgba(255,255,255,.92); backdrop-filter: blur(10px);
      border-bottom: 1px solid var(--line);
      display: flex; align-items: center; justify-content: space-between;
      gap: 1rem; padding: 0 1rem 0 1.1rem;
    }}
    .brand {{ display: flex; flex-direction: column; gap: .1rem; min-width: 0; }}
    .brand h1 {{ font-size: 1rem; margin: 0; letter-spacing: -0.02em; white-space: nowrap; }}
    .brand .meta {{ font-size: .75rem; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    .top-right {{ display: flex; align-items: center; gap: 1rem; flex-wrap: wrap; justify-content: flex-end; }}
    .top-tabs {{ display: flex; gap: .35rem; background: #f1f5f9; padding: .25rem; border-radius: 999px; border: 1px solid var(--line); }}
    .top-tabs .tab {{
      color: var(--muted); text-decoration: none; font-weight: 700; font-size: .85rem;
      padding: .35rem .85rem; border-radius: 999px;
    }}
    .top-tabs .tab:hover {{ color: var(--ink); }}
    .top-tabs .tab.active {{ background: #fff; color: var(--accent); box-shadow: 0 1px 2px rgba(15,23,42,.08); }}
    .renderer-switch {{
      display: flex; align-items: center; gap: .45rem;
      padding: .2rem .35rem .2rem .65rem;
      border: 1px solid var(--line); border-radius: 999px; background: #fff;
    }}
    .renderer-label {{
      font-size: .72rem; font-weight: 700; color: var(--muted);
      text-transform: uppercase; letter-spacing: .04em; white-space: nowrap;
    }}
    .renderer-label strong {{ color: var(--accent); font-size: .78rem; }}
    .renderer-tabs {{ border: 0; background: #f1f5f9; }}
    .renderer-tabs .tab {{ font-size: .78rem; padding: .3rem .7rem; }}
    .renderer-tabs .tab.active {{
      background: #312e81; color: #fff; box-shadow: none;
    }}
    .crumbs {{ display: flex; flex-wrap: wrap; gap: .35rem; align-items: center; font-size: .85rem; }}
    .crumbs a {{ color: var(--accent); text-decoration: none; font-weight: 600; }}
    .crumbs a:hover {{ text-decoration: underline; }}
    .crumbs .sep, .crumbs .here {{ color: var(--muted); }}
    .stage {{
      position: fixed; inset: var(--top) 0 0 0;
      background: #f8fafc;
      overflow: hidden;
      cursor: grab;
      touch-action: none;
    }}
    .stage.grabbing {{ cursor: grabbing; }}
    .board-world {{
      transform-origin: 0 0;
      will-change: transform;
      position: absolute; left: 0; top: 0;
      padding: 1.25rem;
      display: block;
    }}
    .stage .mermaid {{
      display: flex; align-items: center; justify-content: center;
      margin: 0 auto;
    }}
    .stage .mermaid svg {{ max-width: none; height: auto; }}
    .stage > canvas#c4-canvas {{
      position: absolute; inset: 0;
      width: 100% !important; height: 100% !important;
      display: block; border: 0; background: #f8fafc;
    }}
    .board-toolbar {{
      position: fixed; z-index: 35; bottom: 1rem; left: 50%; transform: translateX(-50%);
      display: flex; gap: .35rem; background: rgba(255,255,255,.94);
      border: 1px solid var(--line); border-radius: 999px; padding: .35rem .45rem;
      box-shadow: 0 8px 24px rgba(15,23,42,.12); backdrop-filter: blur(8px);
    }}
    .board-toolbar button {{
      border: 0; background: #f1f5f9; color: var(--ink); font-weight: 700;
      width: 2.2rem; height: 2.2rem; border-radius: 999px; cursor: pointer;
    }}
    .board-toolbar button[data-zoom="reset"] {{ width: auto; padding: 0 .75rem; font-size: .8rem; }}
    .board-toolbar button:hover {{ background: #eef2ff; color: var(--accent); }}
    .board-toolbar .fallback-link {{
      display: inline-flex; align-items: center; height: 2.2rem; padding: 0 .75rem;
      border-radius: 999px; background: #eef2ff; color: var(--accent); font-weight: 700;
      font-size: .78rem; text-decoration: none;
    }}
    .board-toolbar .fallback-link:hover {{ background: #e0e7ff; }}
    .board-toolbar .toolbar-up {{
      display: none;
      align-items: center; justify-content: center;
      height: 2.2rem; min-width: 2.2rem; padding: 0 .65rem;
      border-radius: 999px; background: #0f766e; color: #fff;
      font-weight: 800; font-size: .85rem; text-decoration: none;
      white-space: nowrap; max-width: 9rem; overflow: hidden; text-overflow: ellipsis;
    }}
    .boot-status {{
      position: fixed; z-index: 40; left: 50%; top: calc(var(--top) + 4.5rem);
      transform: translateX(-50%);
      background: rgba(15,23,42,.92); color: #f8fafc;
      padding: .55rem .9rem; border-radius: 10px; font-size: .85rem; font-weight: 600;
      box-shadow: 0 8px 24px rgba(15,23,42,.25); max-width: min(560px, 92vw);
      text-align: center;
    }}
    .boot-status.error {{ background: #b91c1c; }}
    .boot-status[hidden] {{ display: none !important; }}
    .legend-panel {{
      position: fixed; z-index: 30; top: calc(var(--top) + .75rem); left: .75rem;
      width: min(340px, calc(100vw - 1.5rem));
      background: rgba(248,250,252,.94); backdrop-filter: blur(8px);
      border: 1px solid var(--line); border-radius: 12px;
      padding: .55rem .8rem .7rem;
      box-shadow: 0 8px 24px rgba(15,23,42,.08);
    }}
    .legend-panel > summary.legend-head {{
      display: flex; align-items: center; justify-content: space-between;
      gap: .75rem; cursor: pointer; list-style: none;
      user-select: none;
    }}
    .legend-panel > summary.legend-head::-webkit-details-marker {{ display: none; }}
    .legend-panel > summary.legend-head::after {{
      content: "▾"; color: var(--muted); font-size: .85rem; font-weight: 700;
    }}
    .legend-panel[open] > summary.legend-head {{ margin-bottom: .55rem; }}
    .legend-panel[open] > summary.legend-head::after {{ content: "▴"; }}
    .legend-head h2 {{
      margin: 0; font-size: .72rem; letter-spacing: .06em;
      text-transform: uppercase; color: var(--muted); font-weight: 700;
    }}
    .legend-sub {{ font-size: .82rem; color: var(--ink); font-weight: 600; }}
    .legend-list {{
      list-style: none; margin: 0; padding: 0;
      display: grid; gap: .4rem;
    }}
    .legend-item {{
      display: flex; gap: .55rem; align-items: flex-start;
      font-size: .82rem; color: var(--ink);
    }}
    .legend-item .desc {{
      display: block; color: var(--muted); font-size: .74rem; font-weight: 400;
    }}
    .swatch {{
      flex: 0 0 auto;
      display: inline-block; width: 14px; height: 14px; border-radius: 4px;
      margin-top: 2px; border: 1px solid rgba(15,23,42,.12);
    }}
    .swatch.person {{ background: var(--person); }}
    .swatch.system {{ background: var(--system); }}
    .swatch.system-ext {{ background: var(--system-ext); }}
    .swatch.container {{ background: var(--container); }}
    .swatch.component {{ background: var(--component); border-color: #6366f1; }}
    .swatch.code-cls {{ background: #ddd6fe; border-color: #6d28d9; }}
    .swatch.code-iface {{ background: #a7f3d0; border-color: #047857; }}
    .swatch.code-rel {{
      background: linear-gradient(135deg, #fff 40%, #94a3b8 40%, #94a3b8 60%, #fff 60%);
      border-color: #64748b;
    }}
    .drills {{
      position: fixed; z-index: 30; top: calc(var(--top) + .75rem); right: .75rem;
      width: min(280px, calc(100vw - 1.5rem));
      background: rgba(255,255,255,.94); backdrop-filter: blur(8px);
      border: 1px solid var(--line); border-radius: 12px;
      padding: .55rem .8rem .75rem;
      box-shadow: 0 8px 24px rgba(15,23,42,.08);
      max-height: calc(100vh - var(--top) - 1.5rem); overflow: auto;
    }}
    .drills > summary {{
      cursor: pointer; list-style: none; user-select: none;
      display: flex; align-items: center; justify-content: space-between;
    }}
    .drills > summary::-webkit-details-marker {{ display: none; }}
    .drills > summary::after {{
      content: "▾"; color: var(--muted); font-size: .85rem; font-weight: 700;
    }}
    .drills[open] > summary {{ margin-bottom: .55rem; }}
    .drills[open] > summary::after {{ content: "▴"; }}
    .drills h2 {{
      margin: 0; font-size: .72rem; text-transform: uppercase;
      letter-spacing: .06em; color: var(--muted);
    }}
    .drills.sources {{ top: auto; bottom: 4.5rem; max-height: 36vh; }}
    .drills ul {{ list-style: none; margin: 0; padding: 0; display: grid; gap: .45rem; }}
    a.drill {{
      display: grid; grid-template-columns: auto 1fr auto; gap: .45rem; align-items: center;
      padding: .55rem .65rem; border-radius: 8px; border: 1px solid var(--line);
      text-decoration: none; color: var(--ink); background: #f8fafc;
    }}
    a.drill:hover {{ border-color: var(--accent); background: #eef2ff; }}
    a.drill .kind {{
      font-size: .62rem; text-transform: uppercase; letter-spacing: .04em;
      color: #fff; background: var(--accent); padding: .12rem .35rem; border-radius: 4px; font-weight: 700;
    }}
    a.drill .name {{ font-weight: 600; font-size: .9rem; }}
    a.drill .chev {{ color: var(--accent); font-weight: 700; }}
    a.drill.up .kind {{ background: #0f766e; }}
    .drills.drill-up {{
      right: .75rem;
      top: calc(var(--top) + .75rem);
    }}
    .drills:not(.drill-up):not(.sources) {{
      top: calc(var(--top) + 5.5rem);
    }}
    .drill-up-chip {{
      display: none;
      position: fixed; z-index: 36;
      align-items: center; gap: .4rem;
      padding: .45rem .75rem;
      border-radius: 999px;
      background: #0f766e; color: #fff;
      text-decoration: none; font-weight: 700; font-size: .8rem;
      box-shadow: 0 8px 20px rgba(15,23,42,.18);
      border: 1px solid rgba(255,255,255,.2);
      max-width: min(92vw, 360px);
    }}
    .drill-up-chip .kind {{
      font-size: .62rem; text-transform: uppercase; letter-spacing: .04em;
      background: rgba(255,255,255,.2); padding: .1rem .35rem; border-radius: 4px;
    }}
    .drill-up-chip .name {{
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 14rem;
    }}
    .drill-up-chip .chev {{ font-size: 1rem; line-height: 1; }}
    code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .85em; }}
    @media (max-width: 720px) {{
      :root {{ --top: 0px; }}
      body.app-shell {{
        overflow: hidden;
        display: flex; flex-direction: column;
        height: 100dvh; height: 100vh;
      }}
      .topbar {{
        position: sticky; top: 0; inset: auto;
        height: auto; flex-shrink: 0;
        flex-wrap: wrap; align-items: flex-start;
        gap: .4rem .55rem; padding: .45rem .6rem;
      }}
      .brand {{ flex: 1 1 auto; min-width: 0; max-width: 100%; }}
      .brand h1 {{ font-size: .9rem; }}
      .brand .meta {{ font-size: .68rem; max-width: 100%; }}
      .top-right {{
        width: 100%; justify-content: flex-start; gap: .4rem;
        flex-wrap: wrap;
      }}
      .crumbs {{ display: none; }}
      .top-tabs {{ flex: 1 1 auto; overflow-x: auto; -webkit-overflow-scrolling: touch; }}
      .top-tabs .tab {{ font-size: .72rem; padding: .32rem .55rem; white-space: nowrap; }}
      .renderer-switch {{
        flex: 0 0 auto; padding: .12rem;
        order: 3;
      }}
      .renderer-label {{ display: none; }}
      .renderer-tabs .tab {{ font-size: .72rem; padding: .28rem .55rem; }}
      .stage {{
        position: relative; inset: auto; flex: 1 1 auto;
        margin: 0; min-height: 0; width: 100%;
      }}
      .stage .mermaid {{ padding-top: .5rem; display: block; }}
      /* Closed overlays = tiny pills. Open = bottom sheet. Never a huge empty card. */
      details.legend-panel:not([open]),
      details.drills:not([open]) {{
        top: auto !important;
        bottom: calc(5.35rem + env(safe-area-inset-bottom, 0px));
        width: max-content !important;
        max-width: calc(100vw - 1rem) !important;
        max-height: none !important;
        height: auto !important;
        padding: .45rem .75rem !important;
        border-radius: 999px;
        overflow: hidden;
      }}
      details.legend-panel:not([open]) {{ left: .5rem; right: auto; }}
      details.drills:not([open]) {{ right: .5rem; left: auto; }}
      details.drills.sources:not([open]) {{
        right: .5rem; left: auto;
        bottom: calc(8.1rem + env(safe-area-inset-bottom, 0px));
      }}
      details.legend-panel[open],
      details.drills[open] {{
        left: .5rem !important;
        right: .5rem !important;
        bottom: calc(5.35rem + env(safe-area-inset-bottom, 0px)) !important;
        top: auto !important;
        width: auto !important;
        max-width: none !important;
        max-height: 42vh;
        border-radius: 16px;
        padding: .65rem .8rem !important;
        z-index: 38;
        overflow: auto;
      }}
      .legend-sub {{ display: none; }}
      .drills.drill-up {{ display: none !important; }}
      .drill-up-chip {{ display: none !important; }}
      .board-toolbar {{
        bottom: calc(.45rem + env(safe-area-inset-bottom, 0px));
        padding: .4rem .5rem;
        gap: .4rem;
        min-height: 3.6rem;
        max-width: calc(100vw - 0.75rem);
        flex-wrap: nowrap;
        overflow-x: auto;
      }}
      .board-toolbar button {{
        width: 3.1rem; height: 3.1rem;
        font-size: 1.4rem; line-height: 1;
        touch-action: manipulation;
        flex: 0 0 auto;
      }}
      .board-toolbar button[data-zoom="reset"] {{
        min-width: 3.8rem; font-size: .95rem; font-weight: 800;
      }}
      .board-toolbar .fallback-link {{
        height: 3.1rem; padding: 0 .85rem; font-size: .85rem; flex: 0 0 auto;
      }}
      .board-toolbar .toolbar-up {{
        display: inline-flex;
        height: 3.1rem; min-width: 3.1rem; padding: 0 .85rem;
        font-size: 1.05rem; flex: 0 0 auto;
      }}
      .boot-status {{ top: 4.5rem; }}
    }}
  </style>
  <script>
    (() => {{
      const desktop = window.matchMedia('(min-width: 721px)');
      const ids = ['c4-legend', 'c4-drills', 'c4-sources', 'c4-drill-up'];
      const syncChrome = () => {{
        const bar = document.querySelector('.topbar');
        if (bar) document.documentElement.style.setProperty('--top', bar.offsetHeight + 'px');
        const openDesktop = desktop.matches;
        for (const id of ids) {{
          const el = document.getElementById(id);
          if (!el) continue;
          if (openDesktop) {{
            // Desktop: open useful panels; drill-up stays open (one link).
            el.open = (id === 'c4-drill-up' || id === 'c4-drills' || id === 'c4-legend');
          }} else {{
            // Mobile: ALWAYS closed — diagram gets the screen.
            el.open = false;
          }}
        }}
      }};
      const accordion = () => {{
        for (const id of ids) {{
          const el = document.getElementById(id);
          if (!el) continue;
          el.addEventListener('toggle', () => {{
            if (!el.open || desktop.matches) return;
            for (const otherId of ids) {{
              if (otherId === id) continue;
              const other = document.getElementById(otherId);
              if (other) other.open = false;
            }}
          }});
        }}
      }};
      window.addEventListener('DOMContentLoaded', () => {{ syncChrome(); accordion(); }});
      window.addEventListener('resize', () => {{
        const bar = document.querySelector('.topbar');
        if (bar) document.documentElement.style.setProperty('--top', bar.offsetHeight + 'px');
      }});
      desktop.addEventListener('change', syncChrome);
    }})();
  </script>
</head>
<body class="app-shell">
  <header class="topbar">
    <div class="brand">
      <h1>C4 · {layer_title}</h1>
      <div class="meta">render <strong>{renderer_label}</strong></div>
    </div>
    <div class="top-right">
      {crumbs}
      {top_nav}
    </div>
  </header>
  {legend}
  {drills}
  <main class="stage">
    {stage_inner}
  </main>
</body>
</html>
"#,
        mermaid_boot = mermaid_boot,
        layer_title = html_escape(if mode_all { "all" } else { layer.as_str() }),
        renderer_label = renderer_label,
        top_nav = top_nav,
        crumbs = crumbs,
        legend = legend,
        stage_inner = stage_inner,
        drills = drill_html,
    )
}

/// Shared chrome CSS for ADR pages (must match diagram viewer tokens).
fn viewer_shell_css() -> &'static str {
    r#"
    :root {
      --bg: #f1f5f9; --panel: #ffffff; --ink: #0f172a; --muted: #64748b;
      --line: #e2e8f0; --accent: #4f46e5;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif;
      background: var(--bg); color: var(--ink); line-height: 1.5;
    }
    header.topbar {
      background: rgba(255,255,255,.95); border-bottom: 1px solid var(--line);
      padding: .75rem 1.25rem; display: flex; flex-wrap: wrap;
      justify-content: space-between; align-items: center; gap: 1rem;
      position: sticky; top: 0; z-index: 20; backdrop-filter: blur(8px);
    }
    h1 { margin: 0; font-size: 1.1rem; letter-spacing: -0.02em; }
    a { color: var(--accent); text-decoration: none; font-weight: 600; }
    a:hover { text-decoration: underline; }
    .mono, code {
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .85em;
    }
    .mono { color: var(--muted); }
    .top-tabs { display: flex; gap: .35rem; background: #f1f5f9; padding: .25rem; border-radius: 999px; border: 1px solid var(--line); }
    .top-tabs .tab {
      color: var(--muted); text-decoration: none; font-weight: 700; font-size: .85rem;
      padding: .35rem .85rem; border-radius: 999px;
    }
    .top-tabs .tab.active { background: #fff; color: var(--accent); box-shadow: 0 1px 2px rgba(15,23,42,.08); }
    .nav-links { display: flex; flex-wrap: wrap; gap: .75rem 1.25rem; align-items: center; }
    .status {
      font-size: .72rem; font-weight: 700; text-transform: uppercase;
      padding: .2rem .55rem; border-radius: 999px; background: #eef2ff; color: var(--accent);
    }
    .status.accepted { background: #dcfce7; color: #166534; }
    .status.proposed { background: #fef3c7; color: #92400e; }
    .status.rejected { background: #fee2e2; color: #991b1b; }
    .status.superseded, .status.deprecated { background: #f1f5f9; color: #475569; }
    .legend-panel {
      background: #f8fafc; border: 1px solid var(--line); border-radius: 12px;
      padding: .85rem 1rem; margin: 0 0 1.1rem;
    }
    .legend-head {
      display: flex; align-items: baseline; justify-content: space-between;
      gap: .75rem; margin-bottom: .65rem;
    }
    .legend-head h2 {
      margin: 0; font-size: .78rem; letter-spacing: .06em;
      text-transform: uppercase; color: var(--muted); font-weight: 700;
    }
    .legend-sub { font-size: .85rem; color: var(--ink); font-weight: 600; }
    .legend-list {
      list-style: none; margin: 0; padding: 0;
      display: flex; flex-wrap: wrap; gap: .5rem .85rem;
    }
    .legend-list .status { cursor: default; }
"#
}

/// One workspace row for the `/` project index.
#[derive(Debug, Clone)]
pub struct WorkspaceCard {
    pub id: String,
    pub project_id: String,
    pub ref_name: String,
    pub elements: usize,
    pub relationships: usize,
    pub adrs: usize,
    pub flows: usize,
}

/// Landing page: list projects / workspaces with links into the viewer.
pub fn workspaces_index_html(base_url: &str, cards: &[WorkspaceCard]) -> String {
    let base = base_url.trim_end_matches('/');
    let mut by_project: std::collections::BTreeMap<String, Vec<&WorkspaceCard>> =
        std::collections::BTreeMap::new();
    for c in cards {
        by_project.entry(c.project_id.clone()).or_default().push(c);
    }
    let mut sections = String::new();
    if by_project.is_empty() {
        sections.push_str(
            r#"<div class="card empty-card"><p class="empty">No workspaces yet — create one via <code>checkout_workspace</code>.</p></div>"#,
        );
    } else {
        for (project, list) in &by_project {
            let mut rows = String::new();
            for c in list {
                rows.push_str(&format!(
                    r#"<article class="ws-card">
  <div class="ws-head">
    <h3><a href="{base}/?mode=all&amp;renderer=wasm">{name}</a></h3>
    <span class="pill">{ref_name}</span>
  </div>
  <p class="mono ws-id"><code>{id}</code></p>
  <ul class="stats">
    <li><strong>{el}</strong> elements</li>
    <li><strong>{rel}</strong> relationships</li>
    <li><strong>{adrs}</strong> ADRs</li>
    <li><strong>{flows}</strong> flows</li>
  </ul>
  <nav class="ws-links" aria-label="Open workspace">
    <a class="btn primary" href="{base}/?mode=all&amp;renderer=wasm">All (WASM)</a>
    <a class="btn" href="{base}/?layer=context">Context</a>
    <a class="btn" href="{base}/flows">Flows</a>
    <a class="btn" href="{base}/adrs">ADRs</a>
  </nav>
</article>"#,
                    base = base,
                    name = html_escape(&c.id),
                    ref_name = html_escape(&c.ref_name),
                    id = html_escape(&c.id),
                    el = c.elements,
                    rel = c.relationships,
                    adrs = c.adrs,
                    flows = c.flows,
                ));
            }
            sections.push_str(&format!(
                r#"<section class="project">
  <header class="project-head">
    <h2>{project}</h2>
    <span class="mono">{n} workspace(s)</span>
  </header>
  <div class="ws-grid">{rows}</div>
</section>"#,
                project = html_escape(project),
                n = list.len(),
                rows = rows,
            ));
        }
    }
    let shell = viewer_shell_css();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Projects — architect-c4</title>
  <style>
    {shell}
    main {{ max-width: 1100px; margin: 0 auto; padding: 1.25rem 1.5rem 2.5rem; }}
    .project {{ margin-bottom: 1.75rem; }}
    .project-head {{
      display: flex; align-items: baseline; justify-content: space-between;
      gap: .75rem; margin: 0 0 .85rem;
    }}
    .project-head h2 {{
      margin: 0; font-size: 1.05rem; letter-spacing: -0.02em;
    }}
    .ws-grid {{
      display: grid; gap: .85rem;
      grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    }}
    .ws-card, .empty-card {{
      background: var(--panel); border: 1px solid var(--line); border-radius: 16px;
      padding: 1rem 1.1rem 1.1rem;
      box-shadow: 0 1px 2px rgba(15,23,42,.04), 0 8px 24px rgba(15,23,42,.04);
    }}
    .ws-head {{
      display: flex; align-items: flex-start; justify-content: space-between; gap: .5rem;
    }}
    .ws-head h3 {{ margin: 0; font-size: 1rem; word-break: break-word; }}
    .ws-head a {{ color: var(--ink); }}
    .ws-head a:hover {{ color: var(--accent); }}
    .pill {{
      flex: 0 0 auto; font-size: .7rem; font-weight: 700; text-transform: uppercase;
      letter-spacing: .04em; padding: .2rem .55rem; border-radius: 999px;
      background: #eef2ff; color: var(--accent);
    }}
    .ws-id {{ margin: .35rem 0 .75rem; }}
    .stats {{
      list-style: none; margin: 0 0 1rem; padding: 0;
      display: grid; grid-template-columns: 1fr 1fr; gap: .35rem .75rem;
      font-size: .85rem; color: var(--muted);
    }}
    .stats strong {{ color: var(--ink); }}
    .ws-links {{ display: flex; flex-wrap: wrap; gap: .4rem; }}
    .btn {{
      display: inline-block; font-size: .8rem; font-weight: 700;
      padding: .4rem .7rem; border-radius: 999px; border: 1px solid var(--line);
      background: #f8fafc; color: var(--ink); text-decoration: none;
    }}
    .btn:hover {{ border-color: var(--accent); color: var(--accent); text-decoration: none; }}
    .btn.primary {{ background: var(--accent); border-color: var(--accent); color: #fff; }}
    .btn.primary:hover {{ filter: brightness(1.05); color: #fff; }}
    .empty {{ color: var(--muted); text-align: center; margin: 1.5rem 0; }}
    @media (max-width: 640px) {{
      main {{ padding: 1rem; }}
      .stats {{ grid-template-columns: 1fr 1fr; }}
    }}
  </style>
</head>
<body>
  <header class="topbar">
    <div>
      <h1>Projects</h1>
      <div class="mono">{n} workspace(s) · architect-c4</div>
    </div>
    <nav class="top-tabs" aria-label="Primary">
      <a class="tab" href="{base}/?layer=context">Diagrams</a>
    </nav>
  </header>
  <main>
    {sections}
  </main>
</body>
</html>"#,
        shell = shell,
        base = base,
        n = cards.len(),
        sections = sections,
    )
}

pub fn adrs_index_html(_workspace_id: &str, base_url: &str, decisions: &[Decision]) -> String {
    let base = base_url.trim_end_matches('/');
    let mut rows = String::new();
    if decisions.is_empty() {
        rows.push_str(
            r#"<tr><td colspan="5" class="empty">No ADRs yet — use <code>upsert_adr</code></td></tr>"#,
        );
    } else {
        for d in decisions {
            rows.push_str(&format!(
                r#"<tr>
                  <td><a href="{base}/adrs/{id}"><code>{id}</code></a></td>
                  <td><a class="title" href="{base}/adrs/{id}">{title}</a></td>
                  <td><span class="status {st}">{st}</span></td>
                  <td><code>{scope}</code></td>
                  <td class="mono">{date}</td>
                </tr>"#,
                base = base,
                id = urlencoding(&d.id),
                title = html_escape(&d.title),
                st = html_escape(d.status.as_str()),
                scope = html_escape(d.scope_element_id.as_deref().unwrap_or("—")),
                date = html_escape(&d.decided_at),
            ));
        }
    }
    let shell = viewer_shell_css();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>ADRs</title>
  <style>
    {shell}
    main {{ max-width: 1100px; margin: 0 auto; padding: 1.25rem 1.5rem 2rem; }}
    .card {{
      background: var(--panel); border: 1px solid var(--line); border-radius: 16px;
      overflow: hidden;
      box-shadow: 0 1px 2px rgba(15,23,42,.04), 0 8px 24px rgba(15,23,42,.04);
    }}
    table {{ width: 100%; border-collapse: collapse; }}
    th, td {{ text-align: left; padding: .8rem 1rem; border-bottom: 1px solid var(--line); vertical-align: top; }}
    th {{
      font-size: .72rem; text-transform: uppercase; letter-spacing: .06em;
      color: var(--muted); background: #f8fafc;
    }}
    tr:last-child td {{ border-bottom: none; }}
    tr:hover td {{ background: #f8fafc; }}
    a.title {{ color: var(--ink); }}
    a.title:hover {{ color: var(--accent); }}
    .empty {{ color: var(--muted); text-align: center; padding: 2rem; }}
  </style>
</head>
<body>
  <header class="topbar">
    <div>
      <h1>Architecture Decision Records</h1>
      <div class="mono">{n} ADR(s)</div>
    </div>
    <nav class="top-tabs" aria-label="Primary">
      <a class="tab" href="{base}/?layer=context">Diagrams</a>
      <a class="tab" href="{base}/?mode=all&amp;renderer=wasm">All</a>
      <a class="tab" href="{base}/flows">Flows</a>
      <a class="tab active" href="{base}/adrs">ADRs ({n})</a>
    </nav>
  </header>
  <main>
    <aside class="legend-panel" aria-label="ADR status legend">
      <div class="legend-head"><h2>Legend</h2><span class="legend-sub">Decision status</span></div>
      <ul class="legend-list">
        <li><span class="status draft">draft</span></li>
        <li><span class="status proposed">proposed</span></li>
        <li><span class="status accepted">accepted</span></li>
        <li><span class="status rejected">rejected</span></li>
        <li><span class="status superseded">superseded</span></li>
        <li><span class="status deprecated">deprecated</span></li>
      </ul>
    </aside>
    <div class="card">
      <table>
        <thead><tr><th>ID</th><th>Title</th><th>Status</th><th>Scope</th><th>Date</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>
    </div>
  </main>
</body>
</html>"#,
        shell = shell,
        base = base,
        n = decisions.len(),
        rows = rows,
    )
}

pub fn adr_detail_html(_workspace_id: &str, base_url: &str, decision: &Decision) -> String {
    let base = base_url.trim_end_matches('/');
    let mut body = format!(
        "<h2>Context</h2><p>{}</p><h2>Decision</h2><p>{}</p><h2>Consequences</h2><p>{}</p>",
        html_escape(&decision.context),
        html_escape(&decision.decision),
        html_escape(&decision.consequences),
    );
    if let Some(reason) = decision.reason.as_deref() {
        body.push_str(&format!(
            "<h2>Rejection reason</h2><p>{}</p>",
            html_escape(reason)
        ));
    }
    if let Some(pol) = &decision.policy {
        body.push_str(&format!(
            "<h2>Policy</h2><p>mode <code>{}</code></p><ul>",
            html_escape(pol.mode.as_str())
        ));
        for f in &pol.forbid {
            body.push_str(&format!(
                "<li><code>{} → {}</code> ({}) — {}</li>",
                html_escape(f.from_kind.as_str()),
                html_escape(f.to_kind.as_str()),
                html_escape(f.code.as_str()),
                html_escape(&f.message)
            ));
        }
        body.push_str("</ul>");
    }
    if !decision.related_flows.is_empty() {
        body.push_str("<h2>Related flows</h2><ul class=\"related-flows\">");
        for fid in &decision.related_flows {
            body.push_str(&format!(
                r#"<li><a href="{base}/flows/{id}"><code>{id}</code></a></li>"#,
                base = base,
                id = urlencoding(fid),
            ));
        }
        body.push_str("</ul>");
    }
    if !decision.refs.is_empty() {
        body.push_str("<h2>References</h2><ul class=\"related-flows\">");
        for r in &decision.refs {
            let label = r
                .title
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(r.url.as_str());
            body.push_str(&format!(
                r#"<li><a href="{url}" target="_blank" rel="noopener noreferrer">{label}</a></li>"#,
                url = html_escape(&r.url),
                label = html_escape(label),
            ));
        }
        body.push_str("</ul>");
    }
    let shell = viewer_shell_css();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>ADR {id} — {title}</title>
  <style>
    {shell}
    header {{ align-items: flex-start; }}
    header h1 {{ margin: .4rem 0 0; font-size: 1.4rem; }}
    .meta {{
      color: var(--muted); font-size: .9rem; display: flex; flex-wrap: wrap;
      gap: .65rem 1.25rem; margin-top: .65rem;
    }}
    .meta strong {{ color: var(--ink); }}
    main {{ max-width: 820px; margin: 0 auto; padding: 1.25rem 1.5rem 2.5rem; }}
    article {{
      background: var(--panel); border: 1px solid var(--line); border-radius: 16px;
      padding: 1.35rem 1.5rem;
      box-shadow: 0 1px 2px rgba(15,23,42,.04), 0 8px 24px rgba(15,23,42,.04);
    }}
    article h2 {{ font-size: 1.02rem; margin: 1.35rem 0 .45rem; color: var(--ink); }}
    article p {{ margin: .4rem 0; color: #334155; }}
    article ul.related-flows {{ margin: .4rem 0; padding-left: 1.1rem; }}
    article ul.related-flows a {{ color: var(--accent); font-weight: 600; }}
    .id-row {{ display: flex; flex-wrap: wrap; gap: .5rem; align-items: center; }}
  </style>
</head>
<body>
  <header class="topbar">
    <div>
      <nav class="top-tabs" aria-label="Primary" style="margin-bottom:.45rem;display:inline-flex">
        <a class="tab" href="{base}/?layer=context">Diagrams</a>
        <a class="tab" href="{base}/?mode=all&amp;renderer=wasm">All</a>
        <a class="tab" href="{base}/flows">Flows</a>
        <a class="tab active" href="{base}/adrs">ADRs</a>
      </nav>
      <div class="id-row"><code>{id}</code> <span class="status {status}">{status}</span></div>
      <h1>{title}</h1>
      <div class="meta">
        <span>date <strong>{date}</strong></span>
        <span>scope <code>{scope}</code></span>
        <span>commit <code>{commit}</code></span>
        <span>path <code>{path}</code></span>
      </div>
    </div>
  </header>
  <main><article>{body}</article></main>
</body>
</html>"#,
        shell = shell,
        base = base,
        id = html_escape(&decision.id),
        title = html_escape(&decision.title),
        status = html_escape(decision.status.as_str()),
        date = html_escape(&decision.decided_at),
        scope = html_escape(decision.scope_element_id.as_deref().unwrap_or("—")),
        commit = html_escape(decision.git_commit_id.as_deref().unwrap_or("—")),
        path = html_escape(&decision.path),
        body = body,
    )
}

fn breadcrumb_html(
    base: &str,
    _workspace_id: &str,
    layer: C4Layer,
    parent_id: Option<&str>,
    up_parent_id: Option<&str>,
    elements: &[Element],
) -> String {
    let find = |id: &str| elements.iter().find(|e| e.id == id);
    let mut parts = vec![format!(
        r#"<a href="{base}/?layer=context">Context</a>"#,
        base = base
    )];
    match layer {
        C4Layer::Container => {
            parts.push(r#"<span class="sep">/</span>"#.into());
            parts.push(r#"<span class="here">Container</span>"#.into());
            if let Some(p) = parent_id {
                let name = find(p).map(|e| e.name.as_str()).unwrap_or(p);
                parts.push(format!(" <code>{}</code>", html_escape(name)));
            }
        }
        C4Layer::Component => {
            if let Some(sys) = up_parent_id {
                parts.push(r#"<span class="sep">/</span>"#.into());
                parts.push(format!(
                    r#"<a href="{base}/?layer=container&parent={sys}">Container</a>"#,
                    base = base,
                    sys = urlencoding(sys)
                ));
            }
            parts.push(r#"<span class="sep">/</span>"#.into());
            parts.push(r#"<span class="here">Component</span>"#.into());
            if let Some(p) = parent_id {
                let name = find(p).map(|e| e.name.as_str()).unwrap_or(p);
                parts.push(format!(" <code>{}</code>", html_escape(name)));
            }
        }
        C4Layer::Code => {
            // up_parent_id = container; its parent = software_system
            if let Some(container) = up_parent_id {
                if let Some(sys) = find(container).and_then(|c| c.parent_id.as_deref()) {
                    parts.push(r#"<span class="sep">/</span>"#.into());
                    parts.push(format!(
                        r#"<a href="{base}/?layer=container&parent={sys}">Container</a>"#,
                        base = base,
                        sys = urlencoding(sys)
                    ));
                }
                parts.push(r#"<span class="sep">/</span>"#.into());
                parts.push(format!(
                    r#"<a href="{base}/?layer=component&parent={container}">Component</a>"#,
                    base = base,
                    container = urlencoding(container)
                ));
            }
            parts.push(r#"<span class="sep">/</span>"#.into());
            parts.push(r#"<span class="here">Code</span>"#.into());
            if let Some(p) = parent_id {
                let name = find(p).map(|e| e.name.as_str()).unwrap_or(p);
                parts.push(format!(" <code>{}</code>", html_escape(name)));
            }
        }
        _ => {}
    }
    format!(r#"<nav class="crumbs">{}</nav>"#, parts.join(""))
}

fn sanitize_alias(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.is_empty() {
        s.push_str("id");
    }
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, 'n');
    }
    // Mermaid sequenceDiagram treats these as keywords (case-insensitive).
    // Bare id "Actor" breaks: `Actor->>X` → Expecting 'ACTOR', got 'INVALID'.
    if is_mermaid_reserved_alias(&s) {
        s = format!("p_{s}");
    }
    s
}

fn is_mermaid_reserved_alias(alias: &str) -> bool {
    matches!(
        alias.to_ascii_lowercase().as_str(),
        "participant"
            | "actor"
            | "as"
            | "sequence"
            | "sequencediagram"
            | "loop"
            | "alt"
            | "else"
            | "opt"
            | "par"
            | "and"
            | "critical"
            | "break"
            | "rect"
            | "note"
            | "over"
            | "left"
            | "right"
            | "activate"
            | "deactivate"
            | "title"
            | "autonumber"
            | "box"
            | "create"
            | "destroy"
            | "end"
            | "link"
            | "links"
            | "properties"
            | "details"
            | "statediagram"
            | "statediagramv2"
            | "classdiagram"
            | "flowchart"
            | "graph"
            | "subgraph"
            | "direction"
            | "class"
            | "interface"
            | "namespace"
            | "style"
            | "classdef"
            | "click"
            | "callback"
    )
}

/// Escape only characters that break HTML text nodes; keep `>` so `->>` stays valid
/// if a consumer ever reads innerHTML instead of textContent.
fn escape_mermaid_pre(src: &str) -> String {
    src.replace('&', "&amp;").replace('<', "&lt;")
}

fn c4_label(s: &str) -> String {
    s.replace(['—', '–'], "-")
        .replace('\\', "\\\\")
        .replace('"', "'")
        .replace('\n', " ")
}

/// Mermaid C4 rejects empty optional string args (`""`); always emit a placeholder.
fn c4_field(s: Option<&str>) -> String {
    let t = s.map(str::trim).unwrap_or("");
    if t.is_empty() {
        "n/a".into()
    } else {
        c4_label(t)
    }
}

/// Public viewer base must be absolute https (blocks javascript:/data: open-redirect).
pub fn normalize_public_base(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("base_url is empty".into());
    }
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err("base_url must start with https://".into());
    }
    if s.contains('@')
        || s.contains('\\')
        || s.contains('\n')
        || s.contains('\r')
        || s.contains('<')
    {
        return Err("base_url contains forbidden characters".into());
    }
    if lower.contains("javascript:") || lower.contains("data:") {
        return Err("base_url scheme not allowed".into());
    }
    Ok(s.to_string())
}

/// Absolute browser links for agents (`get_view_links`).
pub fn view_links(
    _workspace_id: &str,
    base_url: &str,
    elements: &[Element],
    decisions: &[Decision],
) -> Result<serde_json::Value, String> {
    let base = normalize_public_base(base_url)?;
    let view_url = format!("{base}/");
    let mut systems = Vec::new();
    let mut containers = Vec::new();
    let mut components = Vec::new();
    for e in elements {
        match e.kind {
            ElementKind::SoftwareSystem => {
                systems.push(serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "container_url": format!("{base}/?layer=container&parent={}", urlencoding(&e.id)),
                }));
            }
            ElementKind::Container => {
                containers.push(serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "parent_id": e.parent_id,
                    "component_url": format!("{base}/?layer=component&parent={}", urlencoding(&e.id)),
                }));
            }
            ElementKind::Component => {
                components.push(serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "parent_id": e.parent_id,
                    "code_url": format!("{base}/?layer=code&parent={}", urlencoding(&e.id)),
                }));
            }
            _ => {}
        }
    }
    let adrs: Vec<_> = decisions
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id,
                "title": d.title,
                "status": d.status.as_str(),
                "view_url": format!("{base}/adrs/{}", urlencoding(&d.id)),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "base_url": base,
        "view_url": view_url,
        "context_url": format!("{base}/?layer=context"),
        "flows_url": format!("{base}/flows"),
        "adrs_url": format!("{base}/adrs"),
        "systems": systems,
        "containers": containers,
        "components": components,
        "adrs": adrs,
    }))
}

/// Turn a Flow into Mermaid (sequence for c4_dynamic / passthrough body).
pub fn flow_to_mermaid(flow: &Flow, elements: &[Element]) -> String {
    match flow.kind {
        FlowKind::Sequence | FlowKind::State => flow.body.clone().unwrap_or_default(),
        FlowKind::C4Dynamic => {
            let name_of = |id: &str| -> String {
                elements
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| id.to_string())
            };
            let mut ids: Vec<String> = Vec::new();
            for s in &flow.steps {
                if !ids.contains(&s.from_id) {
                    ids.push(s.from_id.clone());
                }
                if !ids.contains(&s.to_id) {
                    ids.push(s.to_id.clone());
                }
            }
            let mut out = String::from("sequenceDiagram\n");
            for id in &ids {
                let alias = sanitize_alias(id);
                let label = c4_label(&name_of(id));
                out.push_str(&format!("  participant {alias} as {label}\n"));
            }
            let mut steps = flow.steps.clone();
            steps.sort_by_key(|s| s.n);
            for s in steps {
                let from = sanitize_alias(&s.from_id);
                let to = sanitize_alias(&s.to_id);
                let label = c4_label(s.label.as_deref().unwrap_or("uses"));
                out.push_str(&format!("  {from}->>{to}: {label}\n"));
            }
            out
        }
    }
}

pub fn flows_index_html(
    _workspace_id: &str,
    base_url: &str,
    flows: &[Flow],
    adr_count: usize,
) -> String {
    let base = base_url.trim_end_matches('/');
    let mut rows = String::new();
    if flows.is_empty() {
        rows.push_str(
            r#"<tr><td colspan="5" class="empty">No flows yet — use <code>upsert_flow</code></td></tr>"#,
        );
    } else {
        for f in flows {
            rows.push_str(&format!(
                r#"<tr>
                  <td><a href="{base}/flows/{id}"><code>{id}</code></a></td>
                  <td><a class="title" href="{base}/flows/{id}">{title}</a></td>
                  <td><span class="status proposed">{kind}</span></td>
                  <td><code>{usage}</code></td>
                  <td class="mono">{adrs}</td>
                </tr>"#,
                base = base,
                id = urlencoding(&f.id),
                title = html_escape(&f.title),
                kind = html_escape(f.kind.as_str()),
                usage = html_escape(f.usage_key.as_deref().unwrap_or("—")),
                adrs = html_escape(&f.related_adrs.join(", ")),
            ));
        }
    }
    let shell = viewer_shell_css();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Flows</title>
  <style>
    {shell}
    main {{ max-width: 1100px; margin: 0 auto; padding: 1.25rem 1.5rem 2rem; }}
    .card {{
      background: var(--panel); border: 1px solid var(--line); border-radius: 16px; overflow: hidden;
    }}
    table {{ width: 100%; border-collapse: collapse; }}
    th, td {{ text-align: left; padding: .8rem 1rem; border-bottom: 1px solid var(--line); }}
    th {{ font-size: .72rem; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); background: #f8fafc; }}
    .empty {{ color: var(--muted); text-align: center; padding: 2rem; }}
    a.title {{ color: var(--ink); }}
  </style>
</head>
<body>
  <header class="topbar">
    <div>
      <h1>Flows</h1>
      <div class="mono">{n} flow(s)</div>
    </div>
    <nav class="top-tabs" aria-label="Primary">
      <a class="tab" href="{base}/?layer=context">Diagrams</a>
      <a class="tab" href="{base}/?mode=all&amp;renderer=wasm">All</a>
      <a class="tab active" href="{base}/flows">Flows ({n})</a>
      <a class="tab" href="{base}/adrs">ADRs ({na})</a>
    </nav>
  </header>
  <main>
    <div class="card">
      <table>
        <thead><tr><th>ID</th><th>Title</th><th>Kind</th><th>Usage key</th><th>ADRs</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>
    </div>
  </main>
</body>
</html>"#,
        shell = shell,
        base = base,
        n = flows.len(),
        na = adr_count,
        rows = rows,
    )
}

pub fn flow_detail_html(
    _workspace_id: &str,
    base_url: &str,
    flow: &Flow,
    elements: &[Element],
    adr_count: usize,
    flow_count: usize,
) -> String {
    let base = base_url.trim_end_matches('/');
    let mermaid = escape_mermaid_pre(&flow_to_mermaid(flow, elements));
    let theme = mermaid_theme_vars(C4Layer::Context);
    let mut seen = std::collections::HashSet::new();
    let mut links = String::new();
    let mut push_el = |e: &Element| {
        if !seen.insert(e.id.clone()) {
            return;
        }
        let href = match e.kind {
            ElementKind::Person | ElementKind::SoftwareSystem | ElementKind::External => {
                format!("{base}/?layer=context")
            }
            ElementKind::Container => format!(
                "{base}/?layer=container&parent={}",
                urlencoding(e.parent_id.as_deref().unwrap_or(""))
            ),
            ElementKind::Component => format!(
                "{base}/?layer=component&parent={}",
                urlencoding(e.parent_id.as_deref().unwrap_or(""))
            ),
            ElementKind::Code => format!(
                "{base}/?layer=code&parent={}",
                urlencoding(e.parent_id.as_deref().unwrap_or(""))
            ),
        };
        links.push_str(&format!(
            r#"<li><a class="drill" href="{href}"><span class="kind">{kind}</span><span class="name">{name}</span><span class="chev">→</span></a></li>"#,
            href = href,
            kind = html_escape(e.kind.as_str()),
            name = html_escape(&e.name),
        ));
    };
    for s in &flow.steps {
        for id in [&s.from_id, &s.to_id] {
            if let Some(e) = elements.iter().find(|e| e.id == *id) {
                push_el(e);
            }
        }
    }
    for a in &flow.anchors {
        if let Some(eid) = a.element_id.as_deref() {
            if let Some(e) = elements.iter().find(|e| e.id == eid) {
                push_el(e);
            }
        }
    }
    for adr in &flow.related_adrs {
        links.push_str(&format!(
            r#"<li><a class="drill" href="{base}/adrs/{id}"><span class="kind">adr</span><span class="name">{id}</span><span class="chev">↗</span></a></li>"#,
            base = base,
            id = urlencoding(adr),
        ));
    }
    for r in &flow.refs {
        let label = r
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(r.url.as_str());
        links.push_str(&format!(
            r#"<li><a class="drill" href="{url}" target="_blank" rel="noopener noreferrer"><span class="kind">ref</span><span class="name">{label}</span><span class="chev">↗</span></a></li>"#,
            url = html_escape(&r.url),
            label = html_escape(label),
        ));
    }
    let shell = viewer_shell_css();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Flow {id} — {title}</title>
  <style>
    {shell}
    main {{ max-width: 960px; margin: 0 auto; padding: 1rem 1.25rem 5rem; }}
    .meta {{ color: var(--muted); font-size: .9rem; display: flex; flex-wrap: wrap; gap: .5rem 1.25rem; margin: .5rem 0 1rem; }}
    .board {{ background: var(--panel); border: 1px solid var(--line); border-radius: 16px; padding: 1rem; overflow: auto; }}
    .drills {{ position: static; width: auto; margin-top: 1rem; max-height: none; }}
  </style>
</head>
<body class="app-shell" style="overflow:auto">
  <header class="topbar" style="position:sticky">
    <div>
      <div class="id-row"><code>{id}</code> <span class="status proposed">{kind}</span></div>
      <h1 style="margin:.35rem 0 0;font-size:1.25rem">{title}</h1>
      <div class="meta">
        <span>usage <code>{usage}</code></span>
        <span>scope <code>{scope}</code></span>
      </div>
    </div>
    <nav class="top-tabs" aria-label="Primary">
      <a class="tab" href="{base}/?layer=context">Diagrams</a>
      <a class="tab" href="{base}/flows">Flows ({nf})</a>
      <a class="tab" href="{base}/adrs">ADRs ({na})</a>
    </nav>
  </header>
  <main>
    <div class="board"><pre class="mermaid">{mermaid}</pre></div>
    <aside class="drills"><h2>Links</h2><ul>{links}</ul></aside>
  </main>
  <div id="flow-render-error" class="boot-status error" hidden></div>
  <script type="module">
    import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
    const errEl = document.getElementById('flow-render-error');
    try {{
      mermaid.initialize({{ startOnLoad: false, securityLevel: 'loose', theme: 'base', themeVariables: {theme} }});
      await mermaid.run({{ querySelector: '.mermaid' }});
    }} catch (err) {{
      console.error('architect-c4 flow mermaid failed', err);
      if (errEl) {{
        errEl.hidden = false;
        errEl.textContent = 'Mermaid render failed: ' + (err && err.message ? err.message : String(err));
      }}
    }}
  </script>
</body>
</html>"#,
        shell = shell,
        base = base,
        id = html_escape(&flow.id),
        title = html_escape(&flow.title),
        kind = html_escape(flow.kind.as_str()),
        usage = html_escape(flow.usage_key.as_deref().unwrap_or("—")),
        scope = html_escape(flow.scope_element_id.as_deref().unwrap_or("—")),
        nf = flow_count,
        na = adr_count,
        mermaid = mermaid,
        links = if links.is_empty() {
            "<li class=\"empty\">No linked C4/ADR yet</li>".into()
        } else {
            links
        },
        theme = theme,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_index_lists_projects_with_links() {
        let html = workspaces_index_html(
            "https://architecture.example.com",
            &[
                WorkspaceCard {
                    id: "ceph-rados-c4".into(),
                    project_id: "ceph".into(),
                    ref_name: "main".into(),
                    elements: 10,
                    relationships: 5,
                    adrs: 2,
                    flows: 3,
                },
                WorkspaceCard {
                    id: "demo-workspace".into(),
                    project_id: "demo".into(),
                    ref_name: "main".into(),
                    elements: 4,
                    relationships: 1,
                    adrs: 0,
                    flows: 0,
                },
            ],
        );
        assert!(html.contains("Projects"));
        assert!(html.contains(">ceph<"));
        assert!(html.contains("/?mode=all"));
        assert!(html.contains("/?layer=context"));
        assert!(html.contains("10</strong> elements"));
    }

    fn el(id: &str, kind: ElementKind, name: &str) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind,
            parent_id: None,
            name: name.into(),
            description: Some("desc".into()),
            technology: None,
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn context_uses_c4context_not_flowchart() {
        let els = vec![
            el("u", ElementKind::Person, "User"),
            el("s", ElementKind::SoftwareSystem, "Sys"),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "u".into(),
            to_id: "s".into(),
            description: Some("Uses".into()),
            technology: None,
        }];
        let m = overview_mermaid(&DiagramInput {
            elements: &els,
            relationships: &rels,
            base_url: "https://c4.example.com",
        });
        assert!(m.starts_with("C4Context"));
        assert!(m.contains("Person(u,"));
        assert!(m.contains("System(s,"));
        assert!(m.contains("Rel(u, s,"));
        assert!(!m.contains("flowchart"));
    }

    #[test]
    fn container_and_component_c4_types() {
        let mut worker = el("worker", ElementKind::Container, "Worker");
        worker.parent_id = Some("dramatiq".into());
        worker.technology = Some("Python".into());
        let mut actor = el("actor", ElementKind::Component, "Actor");
        actor.parent_id = Some("worker".into());
        let els = vec![
            el("dramatiq", ElementKind::SoftwareSystem, "Dramatiq"),
            worker,
            actor,
        ];
        let input = DiagramInput {
            elements: &els,
            relationships: &[],
            base_url: "https://c4.example.com",
        };
        let cont = diagram_for_layer(&input, C4Layer::Container, Some("dramatiq"));
        assert!(cont.starts_with("C4Container"));
        assert!(cont.contains("Container(worker,"));
        let comp = diagram_for_layer(&input, C4Layer::Component, Some("worker"));
        assert!(comp.starts_with("C4Component"));
        assert!(comp.contains("Component(p_actor,"));
    }

    #[test]
    fn view_html_has_drill_sidebar_and_light_theme() {
        let els = vec![el("dramatiq", ElementKind::SoftwareSystem, "Dramatiq"), {
            let mut c = el("worker", ElementKind::Container, "Worker");
            c.parent_id = Some("dramatiq".into());
            c
        }];
        let m = c4_context(&DiagramInput {
            elements: &els,
            relationships: &[],
            base_url: "https://c4.example.com",
        });
        let html = view_html(
            "w",
            C4Layer::Context,
            None,
            &m,
            "https://c4.example.com",
            None,
            &els,
            3,
            0,
            "layer",
            "mermaid",
            "{}",
        );
        assert!(html.contains("Drill down"));
        assert!(html.contains("layer=container&parent=dramatiq"));
        assert!(html.contains("details class=\"legend-panel\""));
        assert!(html.contains("details class=\"drills\""));
        assert!(html.contains("@media (max-width: 720px)"));
        assert!(html.contains("100dvh") || html.contains("matchMedia('(max-width: 720px)')"));
        assert!(html.contains("aria-label=\"Zoom in\""));
        assert!(html.contains("pinch") && html.contains("pointers"));
        assert!(html.contains("3.1rem") || html.contains("3.25rem"));
        assert!(html.contains("max-content"));
    }

    #[test]
    fn flow_to_mermaid_c4_dynamic() {
        use architect_c4_domain::FlowStep;
        let els = vec![
            el("u", ElementKind::Person, "User"),
            el("api", ElementKind::Container, "API"),
        ];
        let flow = Flow {
            id: "f1".into(),
            workspace_id: "w".into(),
            title: "T".into(),
            kind: FlowKind::C4Dynamic,
            usage_key: None,
            scope_element_id: None,
            related_adrs: vec!["a1".into()],
            epoch: None,
            steps: vec![
                FlowStep {
                    n: 2,
                    from_id: "api".into(),
                    to_id: "u".into(),
                    label: Some("ok".into()),
                },
                FlowStep {
                    n: 1,
                    from_id: "u".into(),
                    to_id: "api".into(),
                    label: Some("req".into()),
                },
            ],
            body: None,
            anchors: vec![],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        };
        let m = flow_to_mermaid(&flow, &els);
        assert!(m.starts_with("sequenceDiagram"));
        assert!(m.contains("participant u as User"));
        assert!(m.contains("u->>api: req"));
        let html = flows_index_html(
            "w",
            "https://c4.example.com",
            std::slice::from_ref(&flow),
            1,
        );
        assert!(html.contains("Flows") && html.contains("f1"));
        let detail = flow_detail_html("w", "https://c4.example.com", &flow, &els, 1, 1);
        assert!(detail.contains("mermaid") && detail.contains("a1"));
    }

    #[test]
    fn drill_up_from_code_to_component() {
        let els = vec![
            el("ceph", ElementKind::SoftwareSystem, "Ceph"),
            {
                let mut c = el("osd", ElementKind::Container, "OSD");
                c.parent_id = Some("ceph".into());
                c
            },
            {
                let mut c = el("objectstore", ElementKind::Component, "ObjectStore");
                c.parent_id = Some("osd".into());
                c
            },
            {
                let mut c = el("BlueStore", ElementKind::Code, "BlueStore");
                c.parent_id = Some("objectstore".into());
                c
            },
        ];
        let up = drill_up_target(&els, C4Layer::Code, Some("objectstore"), false).unwrap();
        assert_eq!(up.layer, C4Layer::Component);
        assert_eq!(up.parent_id.as_deref(), Some("osd"));
        assert_eq!(up.label, "OSD");
        let up2 = drill_up_target(&els, C4Layer::Component, Some("osd"), false).unwrap();
        assert_eq!(up2.layer, C4Layer::Container);
        assert_eq!(up2.parent_id.as_deref(), Some("ceph"));
        let up3 = drill_up_target(&els, C4Layer::Container, Some("ceph"), false).unwrap();
        assert_eq!(up3.layer, C4Layer::Context);
        assert!(up3.parent_id.is_none());
        let html = view_html(
            "ceph-rados-c4",
            C4Layer::Code,
            Some("objectstore"),
            "classDiagram\n",
            "https://c4.example.com",
            Some("osd"),
            &els,
            0,
            0,
            "layer",
            "mermaid",
            "{}",
        );
        assert!(html.contains("Drill up"));
        assert!(html.contains("toolbar-up"));
        assert!(html.contains("layer=component&parent=osd"));
        assert!(html.contains("layer=container&parent=ceph")); // breadcrumb
        assert!(html.contains("c4-drill-up"));
        assert!(html.contains("max-content"));
    }

    #[test]
    fn all_layers_uses_container_subgraphs() {
        let mut api = el("api", ElementKind::Container, "API");
        api.parent_id = Some("sys".into());
        let mut h = el("h", ElementKind::Component, "Handler");
        h.parent_id = Some("api".into());
        let m = all_layers_mermaid(
            &DiagramInput {
                elements: &[el("sys", ElementKind::SoftwareSystem, "Sys"), api, h],
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            None,
        );
        assert!(m.contains("subgraph"));
        assert!(m.contains("subgraph api["));
        assert!(m.contains(":::component") || m.contains("Handler"));
        assert!(m.contains("direction LR"));
        assert!(m.contains("style api "));
        assert!(m.contains("style sys "));
        assert!(!m.contains("_box"));
        assert!(m.contains("Handler") || m.contains("h[\"Handler\"]"));
    }

    #[test]
    fn all_layers_edge_labels_escape_mermaid_specials() {
        let mut api = el("api", ElementKind::Container, "API");
        api.parent_id = Some("sys".into());
        let mut tools = el("tools", ElementKind::Component, "Tools");
        tools.parent_id = Some("api".into());
        let mut view = el("view", ElementKind::Component, "View");
        view.parent_id = Some("api".into());
        let m = all_layers_mermaid(
            &DiagramInput {
                elements: &[
                    el("sys", ElementKind::SoftwareSystem, "Sys"),
                    api,
                    tools,
                    view,
                ],
                relationships: &[Relationship {
                    id: "r".into(),
                    workspace_id: "w".into(),
                    from_id: "tools".into(),
                    to_id: "view".into(),
                    description: Some("get_*_diagram → /* HTML".into()),
                    technology: None,
                }],
                base_url: "https://c4.example.com",
            },
            None,
        );
        // Must not leave raw `*` / `→` inside the |label| segment (breaks Mermaid parse).
        let edge = m
            .lines()
            .find(|l| l.contains("tools") && l.contains("view") && l.contains("-->"))
            .expect("edge line");
        assert!(
            edge.contains("|\"") && edge.contains("\"|"),
            "edge label should be quoted: {edge}"
        );
        assert!(
            !edge.contains("_*_") && !edge.contains('→') && !edge.contains(" /*"),
            "specials must be sanitized: {edge}"
        );
        assert!(
            edge.contains("get_x_diagram") || edge.contains("get_x"),
            "{edge}"
        );
    }

    #[test]
    fn external_marker_stripped_from_system_ext_description() {
        let mut mainframe = el("mf", ElementKind::SoftwareSystem, "Mainframe");
        mainframe.description = Some("external Stores core banking information".into());
        let m = overview_mermaid(&DiagramInput {
            elements: &[el("ibs", ElementKind::SoftwareSystem, "IBS"), mainframe],
            relationships: &[Relationship {
                id: "r".into(),
                workspace_id: "w".into(),
                from_id: "ibs".into(),
                to_id: "mf".into(),
                description: Some("Gets account information from and makes payments using".into()),
                technology: None,
            }],
            base_url: "https://c4.example.com",
        });
        assert!(m.contains("System_Ext(mf,"));
        assert!(m.contains("Stores core banking information"));
        assert!(!m.contains("\"external Stores"));
        // Long Rel labels are truncated so Mermaid C4 doesn't stack text.
        assert!(m.contains("Gets account information from and..."));
        assert!(m.contains("UpdateRelStyle(ibs, mf,"));
        assert!(m.contains("$lineColor=\"#94a3b8\""));
    }

    #[test]
    fn legend_block_for_each_layer() {
        let ctx = legend_block(C4Layer::Context);
        assert!(ctx.contains("legend-panel"));
        assert!(ctx.contains("Person"));
        assert!(ctx.contains("External system"));
        let code = legend_block(C4Layer::Code);
        assert!(code.contains("Class «Cls»") || code.contains("Class"));
        assert!(code.contains("Interface"));
        assert!(code.contains("code-cls"));
        assert!(legend_block(C4Layer::Adr).is_empty());
        assert!(mermaid_theme_vars(C4Layer::Code).contains("#eef2ff"));
        assert!(mermaid_theme_vars(C4Layer::Code).contains("classText"));
        assert!(mermaid_theme_vars(C4Layer::Context).contains("#94a3b8"));
    }

    #[test]
    fn code_view_html_has_legend_and_pastel_theme() {
        let m = "classDiagram\n  direction LR\n  class A {\n    +f()\n  }\n";
        let html = view_html(
            "w",
            C4Layer::Code,
            Some("c"),
            m,
            "https://c4.example.com",
            None,
            &[],
            1,
            0,
            "layer",
            "mermaid",
            "{}",
        );
        assert!(html.contains("legend-panel"));
        assert!(html.contains("Code (UML classDiagram") || html.contains("classDiagram"));
        assert!(html.contains("primaryColor: '#eef2ff'"));
        assert!(html.contains("primaryTextColor: '#1e1b4b'"));
        assert!(html.contains("classText: '#1e1b4b'"));
    }

    #[test]
    fn adrs_index_lists_decisions() {
        use architect_c4_domain::DecisionStatus;
        let d = Decision {
            id: "adr-1".into(),
            workspace_id: "w".into(),
            scope_element_id: Some("worker".into()),
            title: "Use Redis".into(),
            status: DecisionStatus::Accepted,
            decided_at: "2026-07-16".into(),
            context: "Context for decision.".into(),
            decision: "We chose an approach.".into(),
            consequences: "Trade-offs accepted.".into(),
            policy: None,
            related_flows: vec!["rgw-usage-record-on-request".into()],
            refs: vec![],
            reason: None,
            superseded_by_id: None,
            path: "docs/adr/0001.md".into(),
            git_commit_id: Some("abc".into()),
        };
        let html = adrs_index_html("w", "https://c4.example.com", std::slice::from_ref(&d));
        assert!(html.contains("Use Redis"));
        assert!(html.contains("/adrs/adr-1"));
        assert!(html.contains("legend-panel"));
        assert!(html.contains("Decision status"));
        assert!(html.contains("--accent: #4f46e5"));
        assert!(html.contains("--bg: #f1f5f9"));
        assert!(html.contains("top-tabs"));
        assert!(html.contains("class=\"tab active\"") || html.contains("tab active"));
        let detail = adr_detail_html("w", "https://c4.example.com", &d);
        assert!(detail.contains("Redis"));
        assert!(detail.contains("accepted"));
        assert!(detail.contains("--accent: #4f46e5"));
        assert!(detail.contains("top-tabs"));
        assert!(detail.contains("Diagrams"));
        assert!(detail.contains("Related flows"));
        assert!(detail.contains("/flows/rgw-usage-record-on-request"));
    }

    #[test]
    fn sanitize_alias_ok() {
        assert_eq!(sanitize_alias("a.b/c"), "a_b_c");
        assert_eq!(sanitize_alias("12x"), "n12x");
        assert_eq!(sanitize_alias("Actor"), "p_Actor");
        assert_eq!(sanitize_alias("actor"), "p_actor");
        assert_eq!(sanitize_alias("participant"), "p_participant");
        assert_eq!(sanitize_alias("Broker"), "Broker");
    }

    #[test]
    fn flow_to_mermaid_escapes_reserved_actor_id() {
        use architect_c4_domain::FlowStep;
        let els = vec![
            el("application", ElementKind::Person, "Application"),
            el("Actor", ElementKind::Code, "Actor"),
            el("Broker", ElementKind::Code, "Broker"),
        ];
        let flow = Flow {
            id: "flow_deploy_message".into(),
            workspace_id: "w".into(),
            title: "Deploy".into(),
            kind: FlowKind::C4Dynamic,
            usage_key: None,
            scope_element_id: None,
            related_adrs: vec![],
            epoch: None,
            steps: vec![FlowStep {
                n: 1,
                from_id: "Actor".into(),
                to_id: "Broker".into(),
                label: Some("send".into()),
            }],
            body: None,
            anchors: vec![],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        };
        let m = flow_to_mermaid(&flow, &els);
        assert!(
            m.contains("participant p_Actor as Actor"),
            "reserved Actor must be aliased, got:\n{m}"
        );
        assert!(m.contains("p_Actor->>Broker: send"), "got:\n{m}");
        assert!(
            !m.contains("\n  Actor->>"),
            "bare Actor arrow must not appear:\n{m}"
        );
        let detail = flow_detail_html("w", "https://c4.example.com", &flow, &els, 0, 1);
        // Must keep >> for sequence arrows (do not turn into &gt;&gt;).
        assert!(
            detail.contains("p_Actor->>Broker"),
            "mermaid body must keep >>, got snippet missing"
        );
        assert!(!detail.contains("p_Actor-&gt;&gt;Broker"));
        assert!(detail.contains("flow-render-error"));
    }

    #[test]
    fn empty_component_placeholder_inside_boundary() {
        let els = vec![el("producer", ElementKind::Container, "Producer")];
        let m = diagram_for_layer(
            &DiagramInput {
                elements: &els,
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Component,
            Some("producer"),
        );
        assert!(m.starts_with("C4Component"));
        assert!(m.contains("No components yet"));
        assert!(m.contains("n/a"));
        assert!(!m.contains(", \"\", \"\")"));
        // placeholder inside boundary: Component before closing brace of boundary
        let boundary = m.find("Container_Boundary").expect("boundary");
        let empty_comp = m.find("Component(empty").expect("empty");
        let close = m[boundary..].find("\n  }\n").expect("close") + boundary;
        assert!(
            empty_comp < close,
            "empty component must be inside boundary"
        );
        assert!(!m.contains('—'));
    }

    #[test]
    fn code_layer_emits_class_diagram() {
        let mut actor = el("actor", ElementKind::Code, "Actor");
        actor.parent_id = Some("pipeline".into());
        actor.technology = Some("class".into());
        actor.description = Some("+send(*args);+__call__()".into());
        actor.url = Some("https://github.com/example/repo/blob/main/actor.py".into());
        let mut proxy = el("proxy", ElementKind::Code, "ActorProxy");
        proxy.parent_id = Some("pipeline".into());
        proxy.technology = Some("class".into());
        let rel = Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "proxy".into(),
            to_id: "actor".into(),
            description: Some("extends".into()),
            technology: None,
        };
        let m = diagram_for_layer(
            &DiagramInput {
                elements: &[actor, proxy],
                relationships: &[rel],
                base_url: "https://c4.example.com",
            },
            C4Layer::Code,
            Some("pipeline"),
        );
        assert!(m.starts_with("classDiagram"));
        assert!(m.contains("class p_actor"));
        assert!(m.contains("namespace"));
        // Mermaid 11 cannot parse *args / __dunder__ — must be sanitized.
        assert!(m.contains("+send(args)") || m.contains("+send"));
        assert!(m.contains("+call()") || m.contains("call()"));
        assert!(!m.contains("*args"));
        assert!(!m.contains("__"));
        assert!(!m.contains("<<Cls>>"));
        assert!(m.contains("<|--"));
        assert!(!m.contains("flowchart"));
        assert!(!m.contains("click actor"));
    }

    #[test]
    fn code_prose_description_is_not_class_members() {
        let mut store = el("ObjectStore", ElementKind::Code, "ObjectStore");
        store.parent_id = Some("objectstore".into());
        store.technology = Some("class".into());
        // Prose + path must NOT become class body (was: srcosObjectStore.h garbage).
        store.description = Some("Abstract local store API (src/os/ObjectStore.h)".into());
        let m = diagram_for_layer(
            &DiagramInput {
                elements: std::slice::from_ref(&store),
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Code,
            Some("objectstore"),
        );
        assert!(!m.contains("srcos"));
        assert!(!m.contains("Abstract local store"));
        assert!(m.contains("class ObjectStore") || m.contains("class ObjectStore["));
    }

    #[test]
    fn wasm_scene_json_not_html_entity_escaped() {
        let html = view_html(
            "w",
            C4Layer::Context,
            None,
            "flowchart TB\n  a-->b",
            "https://c4.example.com",
            None,
            &[],
            0,
            0,
            "all",
            "wasm",
            r#"{"mode":"all","nodes":[]}"#,
        );
        assert!(html.contains(r#"id="c4-scene">{"mode":"all""#));
        assert!(!html.contains("&quot;mode&quot;"));
        assert!(html.contains("c4-boot-status"));
        assert!(html.contains("module_or_path"));
        assert!(html.contains("architect_c4_wasm_bg.wasm?"));
        assert!(html.contains("renderer-switch"));
        assert!(html.contains("Render <strong>WASM</strong>"));
        assert!(html.contains("renderer=mermaid"));
        assert!(html.contains("renderer=wasm"));
    }

    #[test]
    fn view_html_escapes_mermaid_angle_brackets() {
        let m = "classDiagram\n  class Actor <<Cls>>\n  Actor <|-- ActorProxy\n";
        let html = view_html(
            "w",
            C4Layer::Code,
            Some("c"),
            m,
            "https://c4.example.com",
            None,
            &[],
            0,
            0,
            "layer",
            "mermaid",
            "{}",
        );
        assert!(html.contains("&lt;&lt;Cls&gt;&gt;"));
        assert!(html.contains("&lt;|--"));
        assert!(!html.contains("class Actor <<Cls>>"));
        assert!(html.contains("mermaid.run"));
    }

    #[test]
    fn sanitize_class_member_strips_stars_and_dunders() {
        assert_eq!(sanitize_class_member("+send(*args)"), "+send(args)");
        assert_eq!(
            sanitize_class_member("+send(message: Message) Message"),
            "+send(message: Message) Message"
        );
        assert_eq!(sanitize_class_member("+__call__()"), "+call()");
        assert_eq!(sanitize_class_member("+encode()"), "+encode()");
        assert_eq!(sanitize_class_member("+create_logger()"), "+createLogger()");
        assert_eq!(sanitize_class_member("+aio_finish()"), "+aioFinish()");
        assert_eq!(class_stereo("class"), "Class");
        assert_eq!(class_stereo("interface"), "Interface");
    }

    #[test]
    fn normalize_public_base_https_only() {
        assert!(normalize_public_base("https://c4.example.com/").is_ok());
        assert!(normalize_public_base("http://evil").is_err());
        assert!(normalize_public_base("javascript:alert(1)").is_err());
        assert!(normalize_public_base("https://x@y").is_err());
    }

    #[test]
    fn empty_container_and_code_placeholders() {
        let els = vec![el("sys", ElementKind::SoftwareSystem, "Sys")];
        let cont = diagram_for_layer(
            &DiagramInput {
                elements: &els,
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Container,
            Some("sys"),
        );
        assert!(cont.contains("No containers yet"));
        assert!(!cont.contains(", \"\", \"\")"));
        let code = diagram_for_layer(
            &DiagramInput {
                elements: &els,
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Code,
            Some("missing"),
        );
        assert!(code.starts_with("classDiagram"));
        assert!(code.contains("Empty"));
        let no_parent = diagram_for_layer(
            &DiagramInput {
                elements: &[],
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Code,
            None,
        );
        assert!(no_parent.contains("pick a component parent"));
        let adr = diagram_for_layer(
            &DiagramInput {
                elements: &[],
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Adr,
            None,
        );
        assert!(adr.contains("list_adrs"));
        let land = diagram_for_layer(
            &DiagramInput {
                elements: &els,
                relationships: &[],
                base_url: "https://c4.example.com",
            },
            C4Layer::Landscape,
            None,
        );
        assert!(land.starts_with("C4Context"));
    }

    #[test]
    fn container_shows_related_person_and_external() {
        let mut api = el("api", ElementKind::Container, "API");
        api.parent_id = Some("sys".into());
        let mut ext = el("pay", ElementKind::SoftwareSystem, "Payments");
        ext.description = Some("external payment gateway".into());
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys"),
            api,
            el("u", ElementKind::Person, "User"),
            ext,
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "u".into(),
                to_id: "api".into(),
                description: Some("Calls".into()),
                technology: Some("HTTPS".into()),
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "api".into(),
                to_id: "pay".into(),
                description: Some("Charges".into()),
                technology: None,
            },
        ];
        let m = diagram_for_layer(
            &DiagramInput {
                elements: &els,
                relationships: &rels,
                base_url: "https://c4.example.com",
            },
            C4Layer::Container,
            Some("sys"),
        );
        assert!(m.contains("Person(u,"));
        assert!(m.contains("System_Ext(pay,") || m.contains("System(pay,"));
        assert!(m.contains("Rel("));
    }

    #[test]
    fn code_implements_and_dependency_edges() {
        let mut iface = el("iface", ElementKind::Code, "Broker");
        iface.parent_id = Some("comp".into());
        iface.technology = Some("interface".into());
        let mut impl_ = el("redis", ElementKind::Code, "RedisBroker");
        impl_.parent_id = Some("comp".into());
        impl_.technology = Some("class".into());
        impl_.description = Some("+enqueue()\n+ack()".into());
        let mut other = el("util", ElementKind::Code, "Util");
        other.parent_id = Some("comp".into());
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "redis".into(),
                to_id: "iface".into(),
                description: Some("implements".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "redis".into(),
                to_id: "util".into(),
                description: Some("uses".into()),
                technology: None,
            },
        ];
        let m = diagram_for_layer(
            &DiagramInput {
                elements: &[iface, impl_, other],
                relationships: &rels,
                base_url: "https://c4.example.com",
            },
            C4Layer::Code,
            Some("comp"),
        );
        assert!(m.contains("<|..") || m.contains("..>"));
        assert!(m.contains("+enqueue()"));
        let html = view_html(
            "w",
            C4Layer::Code,
            Some("comp"),
            &m,
            "https://c4.example.com",
            Some("api"),
            &[],
            0,
            0,
            "layer",
            "mermaid",
            "{}",
        );
        assert!(html.contains("classDiagram"));
        assert!(html.contains("ADRs (0)"));
        assert!(html.contains("legend-panel"));
        assert!(html.contains("Code (classDiagram)") || html.contains("classDiagram"));
    }

    #[test]
    fn view_links_builds_absolute_urls() {
        use architect_c4_domain::DecisionStatus;
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys"),
            {
                let mut c = el("api", ElementKind::Container, "API");
                c.parent_id = Some("sys".into());
                c
            },
            {
                let mut c = el("h", ElementKind::Component, "Handler");
                c.parent_id = Some("api".into());
                c
            },
        ];
        let d = Decision {
            id: "adr-1".into(),
            workspace_id: "w".into(),
            scope_element_id: None,
            title: "T".into(),
            status: DecisionStatus::Accepted,
            decided_at: "2026-07-16".into(),
            context: "Context for decision.".into(),
            decision: "We chose an approach.".into(),
            consequences: "Trade-offs accepted.".into(),
            policy: None,
            related_flows: vec![],
            refs: vec![],
            reason: None,
            superseded_by_id: None,
            path: "docs/adr/x.md".into(),
            git_commit_id: None,
        };
        let v = view_links("w", "https://c4.example.com", &els, &[d]).unwrap();
        assert_eq!(
            v["context_url"],
            "https://c4.example.com/?layer=context"
        );
        assert_eq!(
            v["containers"][0]["component_url"],
            "https://c4.example.com/?layer=component&parent=api"
        );
        assert_eq!(
            v["adrs"][0]["view_url"],
            "https://c4.example.com/adrs/adr-1"
        );
        assert!(view_links("w", "javascript:x", &els, &[]).is_err());
    }
}
