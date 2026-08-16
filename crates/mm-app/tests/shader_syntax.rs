//! Does the shader compile at all.
//!
//! WGSL is parsed when a **pipeline** is compiled, which is at draw time, on a machine with a
//! window. So a syntax error or a type that does not check surfaces as a layer that silently does
//! not draw — several minutes and one graphics stack away from the line that was wrong, and
//! indistinguishable from "the feature draws nothing", which is the wrong thing to go looking for.
//!
//! This does the same parse and the same validation naga does inside wgpu, in a second, with no
//! display. It runs in CI on machines that have no GPU at all.
//!
//! # What it does not prove
//!
//! That the shader draws the right picture — `limb_probe` and `shader_probe` are for that — and
//! that the vertex locations match the material's layout, which is a runtime validation against
//! the mesh and cannot be seen from the source alone. It proves the file is a program.

use naga::valid::{Capabilities, ValidationFlags, Validator};

/// Bevy's `#import` is its own preprocessor and not part of WGSL, so naga cannot see through it.
/// Strip the directive and declare what it brought in, so that the rest of the file — which is
/// the part that gets edited — is checked.
///
/// Deliberately dumb: it removes lines from a `#import` to the next line that is exactly `}`, and
/// any single-line `#import`. A shader that grows a form of import this does not understand will
/// fail to parse here, which is the right failure — better a test that says "I do not understand
/// this file" than one that quietly stops checking it.
fn strip_imports(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut skipping = false;
    for line in source.lines() {
        if skipping {
            if line.trim_end() == "}" {
                skipping = false;
            }
            continue;
        }
        if line.starts_with("#import") {
            // A braced block runs on; a one-liner ends here.
            skipping = line.contains('{') && !line.contains('}');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The two `bevy_sprite` helpers the limb shader imports, with the signatures Bevy gives them.
///
/// If Bevy changes one of these the real pipeline breaks and this test does not, which is the
/// known limit of stubbing. It is still worth having: every line of the file that this project
/// actually writes is checked, and those are the lines that change.
const BEVY_STUBS: &str = r#"
fn get_world_from_local(instance_index: u32) -> mat4x4<f32> {
    return mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    );
}

fn mesh2d_position_local_to_clip(world_from_local: mat4x4<f32>, vertex_position: vec4<f32>) -> vec4<f32> {
    return world_from_local * vertex_position;
}
"#;

fn check(name: &str, path: &str) {
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{name}: {e}"));
    let stubbed = format!("{BEVY_STUBS}{}", strip_imports(&source));
    let module = match naga::front::wgsl::parse_str(&stubbed) {
        Ok(module) => module,
        Err(e) => panic!("{name} is not valid WGSL:\n{}", e.emit_to_string(&stubbed)),
    };
    if let Err(e) = Validator::new(ValidationFlags::all(), Capabilities::all()).validate(&module) {
        panic!("{name} does not validate:\n{}", e.emit_to_string(&stubbed));
    }
}

#[test]
fn the_limb_shader_is_a_program() {
    check("limb.wgsl", concat!(env!("CARGO_MANIFEST_DIR"), "/src/limb.wgsl"));
}

#[test]
fn the_cell_shader_is_a_program() {
    // The same two stubs serve: `cell.wgsl` also imports `mesh2d_view_bindings::view`, and does
    // not use it — a dead import, which the strip removes along with the rest of the block.
    check("cell.wgsl", concat!(env!("CARGO_MANIFEST_DIR"), "/src/cell.wgsl"));
}

#[test]
fn every_form_the_mesh_can_emit_is_one_the_shader_knows() {
    // The failure this catches has no other symptom at all: `limbmesh::form_of` hands the shader a
    // number, `field_of` branches on it, and a form the shader has never heard of falls through to
    // a field nothing can reach — so the organelle simply does not draw, exactly as it did before
    // any of this existed, and nothing anywhere says why.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/limb.wgsl"
    ))
    .expect("read limb.wgsl");
    let organelle_forms = mm_core::OrganelleType::all()
        .iter()
        .filter_map(|k| mm_app::limbmesh::form_of(*k).map(|f| (k.name(), f)));
    // The junctions come from `slide::JunctionLine` rather than from an organelle, so they are
    // named here or nothing would check them.
    let junction_forms = [
        ("hard junction", mm_app::limbmesh::form::BAND),
        ("soft junction", mm_app::limbmesh::form::CHANNEL),
    ];
    for (name, form) in organelle_forms.chain(junction_forms) {
        // The constant the shader compares against, by value rather than by name: the names could
        // agree while the numbers do not, and it is the numbers that travel in the vertex buffer.
        let wanted = format!("= {form:.1};");
        assert!(
            source.contains(&wanted),
            "{name} is emitted as form {form} and `limb.wgsl` declares no such constant"
        );
    }
}
