//! CPU reference renderer (default and guaranteed backend, spec §11).
//! Not a fallback that may be removed later.

pub mod points;
pub mod raster;
pub mod shading;

use crate::camera::projection::Fit;
use crate::camera::Camera;
use crate::diagnostics::error::{Error, Result};
use crate::geometry::normals::{orient_toward_camera, recompute_face_normals};
use crate::geometry::types::{Geometry, Mesh, Vec3};
use crate::render::cpu::raster::{rasterize_splat, rasterize_triangle, TriAttrs};
use crate::render::image::Framebuffer;
use crate::render::matcap::Matcap;

/// Backend selection. MVP: `auto` and `cpu` resolve to CPU; `gpu` must
/// fail clearly (spec §15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Cpu,
    Gpu,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Cpu => "cpu",
            Backend::Gpu => "gpu",
        }
    }
}

impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Backend::Auto),
            "cpu" => Ok(Backend::Cpu),
            "gpu" => Ok(Backend::Gpu),
            other => Err(format!(
                "unknown backend '{other}' (expected auto, cpu, or gpu)"
            )),
        }
    }
}

impl Backend {
    /// Resolve a requested backend to the effective backend.
    pub fn resolve(self) -> Result<EffectiveBackend> {
        match self {
            Backend::Auto | Backend::Cpu => Ok(EffectiveBackend::Cpu),
            Backend::Gpu => Err(Error::backend_unavailable(
                "the GPU backend is not implemented in the MVP; use `--backend cpu` or `auto`",
            )),
        }
    }
}

/// The backend actually used for rendering (included in cache identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveBackend {
    Cpu,
}

impl EffectiveBackend {
    pub fn as_str(self) -> &'static str {
        "cpu"
    }
}

/// Rendering inputs shared by both geometry paths.
pub struct Scene {
    pub camera: Camera,
    pub fit: Fit,
    /// View-space forward direction (toward camera) for double-sided
    /// normal orientation.
    pub view_forward: Vec3,
}

/// Render a mesh into the framebuffer using flat or interpolated
/// normals with double-sided winding handling.
pub fn render_mesh(
    fb: &mut Framebuffer,
    matcap: &Matcap,
    scene: &Scene,
    mesh: &Mesh,
) -> Result<()> {
    let face_normals = recompute_face_normals(mesh);
    let mut warnings = mesh.warnings.clone().unwrap_or_default();
    let _ = &mut warnings;

    if let Some(normals) = &mesh.normals {
        // Per-vertex classification, computed once per vertex instead of
        // per-vertex-per-triangle (a smooth mesh re-normalizes and orients
        // the same vertex normal for every triangle sharing it).
        //   Ok(n)   -> normalized vertex normal (use as-is, oriented)
        //   Face    -> non-normalizable but above the length gate (infinite
        //              components): the triangle falls back to its own
        //              face normal at this vertex, exactly as before
        //   Deg     -> below the length gate: forces the flat branch
        #[derive(Clone, Copy)]
        enum VNormal {
            Ok(Vec3),
            Face,
            Deg,
        }
        let vertex_normals: Vec<VNormal> = normals
            .iter()
            .map(|nv| {
                if nv.length_squared() > 1e-12 {
                    match nv.try_normalize() {
                        Some(n) => VNormal::Ok(n),
                        None => VNormal::Face,
                    }
                } else {
                    VNormal::Deg
                }
            })
            .collect();
        let orient_for = |n: Vec3| orient_toward_camera(n, scene.view_forward).normal;

        for (i, tri) in mesh.triangles.iter().enumerate() {
            let Some(face_normal) = face_normals[i] else {
                continue;
            };
            let (ai, bi, ci) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let base = [mesh.positions[ai], mesh.positions[bi], mesh.positions[ci]];
            let (va, vb, vc) = (vertex_normals[ai], vertex_normals[bi], vertex_normals[ci]);
            let oriented: [Vec3; 3] = if matches!(va, VNormal::Deg | VNormal::Face)
                || matches!(vb, VNormal::Deg | VNormal::Face)
                || matches!(vc, VNormal::Deg | VNormal::Face)
            {
                // Any below-gate share degrades the whole triangle to flat.
                if matches!(va, VNormal::Deg)
                    || matches!(vb, VNormal::Deg)
                    || matches!(vc, VNormal::Deg)
                {
                    let on = orient_for(face_normal);
                    [on; 3]
                } else {
                    // Above-gate but non-normalizable: mixed interpolation
                    // using the triangle face normal at those vertices.
                    [
                        match va {
                            VNormal::Ok(n) => orient_for(n),
                            _ => orient_for(face_normal),
                        },
                        match vb {
                            VNormal::Ok(n) => orient_for(n),
                            _ => orient_for(face_normal),
                        },
                        match vc {
                            VNormal::Ok(n) => orient_for(n),
                            _ => orient_for(face_normal),
                        },
                    ]
                }
            } else {
                let VNormal::Ok(na) = va else { unreachable!() };
                let VNormal::Ok(nb) = vb else { unreachable!() };
                let VNormal::Ok(nc) = vc else { unreachable!() };
                [orient_for(na), orient_for(nb), orient_for(nc)]
            };

            let colors = mesh.colors.as_ref().map(|cs| {
                [
                    cs[tri[0] as usize],
                    cs[tri[1] as usize],
                    cs[tri[2] as usize],
                ]
            });
            let screen = project_triangle(&scene.camera, &scene.fit, &base);
            let tri = raster::ScreenTriangle {
                v: screen,
                shading: TriAttrs {
                    normals: oriented,
                    colors,
                },
            };
            rasterize_triangle(fb, matcap, &tri);
        }
    } else {
        // Flat shading path.
        for (i, tri) in mesh.triangles.iter().enumerate() {
            let Some(face_normal) = face_normals[i] else {
                continue;
            };
            let base = [
                mesh.positions[tri[0] as usize],
                mesh.positions[tri[1] as usize],
                mesh.positions[tri[2] as usize],
            ];
            let on = orient_toward_camera(face_normal, scene.view_forward).normal;
            let colors = mesh.colors.as_ref().map(|cs| {
                [
                    cs[tri[0] as usize],
                    cs[tri[1] as usize],
                    cs[tri[2] as usize],
                ]
            });
            let screen = project_triangle(&scene.camera, &scene.fit, &base);
            let tri = raster::ScreenTriangle {
                v: screen,
                shading: TriAttrs {
                    normals: [on; 3],
                    colors,
                },
            };
            rasterize_triangle(fb, matcap, &tri);
        }
    }
    Ok(())
}

/// Project three world-space positions through camera + fit.
fn project_triangle(camera: &Camera, fit: &Fit, v: &[Vec3; 3]) -> [(f32, f32, f32); 3] {
    let mut out = [(0.0f32, 0.0f32, 0.0f32); 3];
    for (i, p) in v.iter().enumerate() {
        let (sx, sy, depth) = camera.to_camera_space(*p);
        let (px, py) = fit.to_pixels(sx, sy);
        out[i] = (px, py, depth);
    }
    out
}

/// Render a point cloud into the framebuffer.
pub fn render_point_cloud(
    fb: &mut Framebuffer,
    matcap: &Matcap,
    scene: &Scene,
    cloud: &crate::geometry::types::PointCloud,
    point_opts: &crate::render::options::PointRender,
    quality: u32,
) -> Result<()> {
    let render = points::PointCloudRender::prepare(
        &scene.camera,
        cloud,
        point_opts,
        quality,
        fb.width,
        fb.height,
    )?;
    for (sx, sy, depth, color) in &render.points {
        let (px, py) = scene.fit.to_pixels(*sx, *sy);
        let n = scene.view_forward; // billboard normal toward camera
        let shaded = crate::render::cpu::shading::shade_point(matcap, n, *color);
        rasterize_splat(fb, px, py, *depth, render.radius, shaded);
    }
    Ok(())
}

/// Detect degenerate triangles that rasterization would drop (diagnostics).
#[allow(dead_code)]
pub fn count_degenerate_faces(mesh: &Mesh) -> usize {
    mesh.warnings
        .as_ref()
        .map(|w| w.degenerate_triangles)
        .unwrap_or(0)
}

/// Render normalized geometry into a framebuffer given a scene.
pub fn render_geometry(
    fb: &mut Framebuffer,
    matcap: &Matcap,
    scene: &Scene,
    geometry: &Geometry,
    point_opts: &crate::render::options::PointRender,
    quality: u32,
) -> Result<()> {
    match geometry {
        Geometry::Mesh(m) => render_mesh(fb, matcap, scene, m),
        Geometry::PointCloud(p) => render_point_cloud(fb, matcap, scene, p, point_opts, quality),
    }
}
